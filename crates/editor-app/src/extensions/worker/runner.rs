//! The production actor keeps dispatching old instances while a separate thread prepares a candidate.
use super::preparation::BackgroundPreparation;
use super::*;
use rust_i18n::t;
use std::collections::VecDeque;

impl Worker {
    /// Run the production actor; tests use the same transport and publications with isolated storage.
    pub(super) fn start_background(root: PathBuf, environment: Environment, trusted: bool) -> Self {
        let (tx, rx) = mpsc::channel();
        let state = Arc::new(Mutex::new(Self::initial_state(
            &root,
            &environment,
            trusted,
        )));
        let output = state.clone();
        let trust = Arc::new(std::sync::atomic::AtomicBool::new(trusted));
        let authority = trust.clone();
        std::thread::spawn(move || {
            // Prepare the public SDK off the UI thread. A failure is delivered only to guests that request it.
            let resources = plugin_runtime::HostResources {
                sdk: Some(crate::sdk_export::descriptor().map_err(|error| format!("{error:#}"))),
                logs: output.lock().unwrap().logs.clone(),
                ..Default::default()
            };
            let mut manager =
                match Manager::open_with_resources(root, environment, trusted, resources) {
                    Ok(m) => m,
                    Err(e) => {
                        let mut published = output.lock().unwrap();
                        published.startup.clear();
                        published.status = Some(OperationStatus {
                            plugin: None,
                            message: format!("{e:#}"),
                        });
                        return;
                    }
                };
            let mut last_save = Instant::now();
            let mut vectors = super::super::images::VectorRenderer::default();
            let mut preparation: Option<BackgroundPreparation> = None;
            // First-use preparation retains its native cancellation token until the serialized cutover.
            let mut bundled_install: Option<super::super::bundled::Candidate> = None;
            let mut deferred = VecDeque::new();
            let mut instance_ids = BTreeMap::<String, String>::new();
            // Host session identity joined to the configuration and launch that requested it.
            let mut run_requests = BTreeMap::<u64, (String, u64)>::new();
            // Real adapter handshakes and inspections remain pending while the actor serves other work.
            let mut debug_requests = BTreeMap::<u64, (String, plugin_runtime::DebugRequest)>::new();
            // A slow status reply stays pending until its own deadline; 500 ms is never an outcome.
            let mut run_polls =
                BTreeMap::<(String, u64), plugin_runtime::Completion<serde_json::Value>>::new();
            // Repeated selection shares one pending location per session, bounded independently of polling.
            let mut run_locations =
                BTreeMap::<(u64, u64), plugin_runtime::Completion<serde_json::Value>>::new();
            let mut target_calls = super::targets::TargetCalls::default();
            let mut configuration_calls = super::configurations::ConfigurationCalls::default();
            loop {
                // Target calls use their own bounded roots and never block the actor on build output.
                // Only the candidate travels between threads. Cutover remains serialized with live dispatch.
                let completed = preparation
                    .as_mut()
                    .and_then(BackgroundPreparation::try_ready)
                    .map(|result| {
                        let mut job = preparation.take().unwrap();
                        job.join_completed();
                        (job.id.clone(), job.control.clone(), result)
                    });
                let mut work = if completed.is_some() {
                    None
                } else if preparation.is_none() && !deferred.is_empty() {
                    deferred.pop_front()
                } else {
                    match rx.recv_timeout(Duration::from_millis(30)) {
                        Ok(work) => Some(work),
                        Err(mpsc::RecvTimeoutError::Timeout) => None,
                        Err(_) => break,
                    }
                };
                if let Some(pending) = &preparation {
                    if bundled_install.as_ref().is_some_and(|candidate| {
                        !candidate.request.is_active()
                            || !authority.load(std::sync::atomic::Ordering::Acquire)
                    }) {
                        pending.control.cancel();
                    }
                    match work.as_ref() {
                        Some(Work::Shutdown(_)) => {
                            pending.control.cancel();
                            {
                                let mut published = output.lock().unwrap();
                                for id in manager.live.keys() {
                                    retire_publication(&mut published, id);
                                }
                            }
                            manager.shutdown();
                            // Acknowledge only after the helper has released its candidate and transaction lock.
                            deferred.push_front(work.take().unwrap());
                        }
                        Some(Work::SetTrust(false)) => {
                            pending.control.cancel();
                            // A later revocation supersedes earlier deferred grants; finishing preparation
                            // cannot restore authority while the native workspace still shows restricted.
                            deferred.retain(|work| !matches!(work, Work::SetTrust(_)));
                        }
                        Some(
                            Work::SetSetting { .. }
                            | Work::ImportPreference { .. }
                            | Work::SetServiceProvider { .. }
                            | Work::Enable(_)
                            | Work::Restart(_)
                            | Work::Disable(_)
                            | Work::SetProjectEnabled(_, _)
                            | Work::SetTrust(true)
                            | Work::Uninstall(_, _)
                            | Work::Install(_)
                            | Work::InspectBundle(_)
                            | Work::InstallBundle(_)
                            | Work::DeclineBundle(_),
                        ) => {
                            // Persistent choices cannot write through the transaction lock. Old commands and
                            // typed requests continue normally until the candidate returns for cutover.
                            deferred.push_back(work.take().unwrap());
                        }
                        _ => {}
                    }
                }
                let mut lifecycle = completed
                    .as_ref()
                    .map(|(id, _, _)| OperationProgress {
                        id: id.clone(),
                        action: LifecycleAction::Install,
                        delete_data: None,
                    })
                    .or_else(|| work.as_ref().and_then(Work::lifecycle));
                // Capture ownership before consuming either an asynchronous install or the queued action.
                let status_plugin = completed
                    .as_ref()
                    .map(|(id, _, _)| id.clone())
                    .or_else(|| work.as_ref().and_then(Work::plugin_id).map(str::to_owned));
                let retiring = if let Some((id, _, Ok(_))) = &completed {
                    vec![id.clone()]
                } else {
                    match &work {
                        Some(Work::SetSetting { plugin, .. }) => vec![plugin.clone()],
                        Some(
                            Work::Restart(id)
                            | Work::Disable(id)
                            | Work::Uninstall(id, _)
                            | Work::SetProjectEnabled(id, false),
                        ) => vec![id.clone()],
                        Some(Work::Install(package)) if package.manifest.component.is_none() => {
                            vec![package.manifest.id.clone()]
                        }
                        Some(Work::SetTrust(false) | Work::Shutdown(_)) => {
                            manager.live.keys().cloned().collect()
                        }
                        _ => Vec::new(),
                    }
                };
                {
                    // Native callbacks can run while commit snapshots/stops the old owner. Seal their epoch
                    // before that blocking turn, including effects already transferred to a UI defer queue.
                    let mut published = output.lock().unwrap();
                    for id in retiring {
                        retire_publication(&mut published, &id);
                    }
                }
                let result = if let Some((_, control, prepared)) = completed {
                    // Taking before inspecting Result releases retained package bytes on preparation failure too.
                    let candidate = bundled_install.take();
                    prepared.and_then(|prepared| {
                        if let Some(candidate) = candidate {
                            // A preparation result cannot restore a withdrawn file or supersede a provider choice.
                            anyhow::ensure!(
                                authority.load(std::sync::atomic::Ordering::Acquire),
                                "Workspace restricted"
                            );
                            super::super::bundled::validate_candidate(&manager, &candidate)?;
                        }
                        manager.commit_installation(prepared, &control)
                    })
                } else {
                    match work.and_then(|work| work.admit(&manager, &output)) {
                        Some(Work::Validated { .. }) => {
                            unreachable!("admission consumes provenance")
                        }
                        Some(
                            work @ (Work::PrepareTarget { .. }
                            | Work::CancelTarget { .. }
                            | Work::DiscoverTargets { .. }),
                        ) => {
                            target_calls.dispatch(work, &mut manager, &output);
                            Ok(())
                        }
                        Some(
                            work @ (Work::ConfigurationCatalog { .. }
                            | Work::ConfigurationCall { .. }
                            | Work::CancelConfigurations { .. }),
                        ) => {
                            configuration_calls.dispatch(work, &mut manager, &output);
                            Ok(())
                        }
                        // A debug call is answered by the provider and published as what it said,
                        // never as a state the host inferred.
                        Some(Work::DebugCall {
                            request,
                            configuration,
                            method,
                            arguments,
                        }) => {
                            let result = if debug_requests.len() >= 128
                                || debug_requests.contains_key(&request)
                            {
                                Err(anyhow::anyhow!(
                                    "Debug request quota exhausted or duplicate identity"
                                ))
                            } else {
                                manager.begin_configured_debug_call(
                                    configuration.as_deref(),
                                    &method,
                                    arguments,
                                )
                            };
                            match result {
                                Ok(pending) => {
                                    if method == "start" {
                                        output.lock().unwrap().debug_answers.push((
                                            request,
                                            DebugAnswerMessage::Connecting(
                                                pending.session().into(),
                                            ),
                                        ));
                                    }
                                    debug_requests.insert(request, (method, pending));
                                }
                                Err(error) => output.lock().unwrap().debug_answers.push((
                                    request,
                                    DebugAnswerMessage::Failed(format!("{error:#}")),
                                )),
                            }
                            Ok(())
                        }
                        // Reading the provider list holds no session and changes no selection, so it
                        // is safe to ask whenever the page that shows providers is opened.
                        Some(Work::ForceDebug { session, request }) => {
                            // Force follows the same asynchronous actual-exit receipt as normal Stop.
                            match manager.force_debug_session(&session) {
                                Ok(pending) => {
                                    debug_requests.insert(request, ("stop".into(), pending));
                                }
                                Err(error) => output.lock().unwrap().debug_answers.push((
                                    request,
                                    DebugAnswerMessage::Failed(format!("{error:#}")),
                                )),
                            }
                            Ok(())
                        }
                        Some(Work::ListRunProviders) => {
                            let providers = manager.execution_providers();
                            let debug = manager.debug_availability();
                            // The abilities come from the same provider the availability answer names,
                            // so what the panel offers and what it may ask for are one fact.
                            let abilities = manager.debug_service_abilities();
                            let mut published = output.lock().unwrap();
                            published.run_providers = Some(providers);
                            published.debug_availability = Some(debug);
                            published.debug_abilities = abilities;
                            published.debug_provider_abilities = manager.all_debug_abilities();
                            Ok(())
                        }
                        Some(Work::SetRunProvider { provider }) => {
                            // The runtime keeps this in its own versioned store, so the choice
                            // outlives the session and every launch path reads the same answer.
                            let scope = settings::Scope::Project;
                            match provider {
                                Some(_) => manager.set_service_provider(
                                    api::InstanceScope::Workspace,
                                    scope,
                                    plugin_runtime::EXECUTION_CONTRACT,
                                    provider.as_deref(),
                                ),
                                // Following the default again means removing the explicit choice,
                                // not picking whichever provider happens to be first.
                                None => manager.set_service_provider(
                                    api::InstanceScope::Workspace,
                                    scope,
                                    plugin_runtime::EXECUTION_CONTRACT,
                                    None,
                                ),
                            }
                            .map_err(|error| anyhow::anyhow!("{error:#}"))
                        }
                        Some(Work::SetServiceProvider {
                            request,
                            owner,
                            scope,
                            contract,
                            provider,
                        }) => {
                            let result = manager.set_service_provider(
                                owner,
                                scope,
                                &contract,
                                provider.as_deref(),
                            );
                            let mut published = output.lock().unwrap();
                            published.configuration_result = Some((
                                request,
                                result
                                    .as_ref()
                                    .map(|_| ())
                                    .map_err(|error| format!("{error:#}")),
                            ));
                            published.configuration_revision += 1;
                            result
                        }
                        Some(Work::SetSetting {
                            request,
                            plugin,
                            scope,
                            key,
                            value,
                        }) => {
                            let result = manager.update_setting(&plugin, scope, &key, value);
                            let mut published = output.lock().unwrap();
                            published.configuration_result = Some((
                                request,
                                result
                                    .as_ref()
                                    .map(|_| ())
                                    .map_err(|error| format!("{error:#}")),
                            ));
                            published.configuration_revision += 1;
                            result
                        }
                        Some(Work::StartRun {
                            request,
                            config,
                            request_id,
                        }) => {
                            // The runtime selects a compatible provider by contract and scope; a
                            // missing or ambiguous provider is reported instead of being guessed at.
                            let result = manager
                                .start_configuration_execution(&config, request)
                                .map(|session| {
                                    // Remember which launch produced this session before it is published.
                                    run_requests.insert(session.id(), (config.clone(), request_id));
                                });
                            if let Err(error) = &result {
                                let mut published = output.lock().unwrap();
                                published.run_errors.push((
                                    config.clone(),
                                    request_id,
                                    format!("{error:#}"),
                                ));
                                published.configuration_revision += 1;
                            }
                            result.map_err(|error| anyhow::anyhow!("{error:#}"))
                        }
                        Some(Work::PollRun {
                            session,
                            config,
                            request_id,
                        }) => {
                            // The ordinary actor poll drives this query without delaying other work.
                            match manager.query_execution(session) {
                                Ok(completion) if run_polls.len() < 128 => {
                                    run_polls.insert((config, request_id), completion);
                                }
                                _ => {
                                    let mut published = output.lock().unwrap();
                                    published.run_status.push((
                                        config,
                                        request_id,
                                        super::RunStatus::Unknown,
                                    ));
                                    published.configuration_revision += 1;
                                }
                            }
                            Ok(())
                        }
                        Some(Work::LocateRun { session, request }) => {
                            let result = if run_locations.contains_key(&(session, request)) {
                                Ok(None)
                            } else if run_locations.len() >= 128 {
                                Err("Too many pending session locations".to_owned())
                            } else {
                                manager
                                    .locate_execution(session)
                                    .map(Some)
                                    .map_err(|error| format!("{error:#}"))
                            };
                            match result {
                                Ok(Some(completion)) => {
                                    run_locations.insert((session, request), completion);
                                }
                                Ok(None) => {}
                                Err(error) => {
                                    output.lock().unwrap().locate_results.push((
                                        session,
                                        request,
                                        Err(error),
                                    ));
                                }
                            }
                            Ok(())
                        }
                        Some(Work::StopRun {
                            session,
                            config,
                            mode,
                            request_id,
                        }) => {
                            // The runtime asks the session's own provider; the worker never holds a
                            // provider-private handle and never terminates a program directly.
                            let result = manager
                                .stop_execution_with(
                                    session,
                                    plugin_runtime::StopOptions {
                                        mode,
                                        ..Default::default()
                                    },
                                )
                                .map_err(|error| format!("{error:#}"));
                            let mut published = output.lock().unwrap();
                            published
                                .stop_results
                                .push((config, request_id, result.clone()));
                            published.configuration_revision += 1;
                            result.map_err(|error| anyhow::anyhow!("{error}"))
                        }
                        Some(Work::Shutdown(ack)) => {
                            drop(manager);
                            if let Some(ack) = ack {
                                let _ = ack.send(());
                            }
                            return;
                        }
                        Some(Work::Inspect(path)) => Package::read(&path)
                            .map(|package| output.lock().unwrap().pending = Some(package)),
                        Some(Work::InspectBundle(request)) => {
                            let result = if authority.load(std::sync::atomic::Ordering::Acquire) {
                                super::super::bundled::prepare_offer(&mut manager, &request)
                            } else {
                                Ok(None)
                            }
                            .map_err(|error| format!("{error:#}"));
                            output.lock().unwrap().bundle_reply =
                                Some(super::super::bundled::Reply {
                                    token: request.token,
                                    result,
                                });
                            Ok(())
                        }
                        Some(Work::DeclineBundle(candidate)) => {
                            manager.record_bundle_decline(&candidate.package.manifest.id)
                        }
                        Some(work @ (Work::Install(_) | Work::InstallBundle(_))) => {
                            let package = match work {
                                Work::Install(package) => Ok((Arc::new(package), None)),
                                Work::InstallBundle(candidate) => {
                                    let trusted =
                                        authority.load(std::sync::atomic::Ordering::Acquire);
                                    if !trusted {
                                        candidate
                                            .request
                                            .active
                                            .store(false, std::sync::atomic::Ordering::Release);
                                    }
                                    super::super::bundled::validate_candidate(&manager, &candidate)
                                        .map(|()| (candidate.package.clone(), Some(candidate)))
                                }
                                _ => unreachable!("only installation work reaches this branch"),
                            };
                            package.and_then(|(package, candidate)| {
                                let control = output
                                    .lock()
                                    .unwrap()
                                    .install_control
                                    .clone()
                                    .unwrap_or_default();
                                let control = if let Some(candidate) = &candidate {
                                    let active = candidate.request.active.clone();
                                    let trusted = authority.clone();
                                    // Resource-only installs are synchronous; every runtime check must see native
                                    // withdrawal directly rather than wait for the actor's next message turn.
                                    control.with_guard(move || {
                                        active.load(std::sync::atomic::Ordering::Acquire)
                                            && trusted.load(std::sync::atomic::Ordering::Acquire)
                                    })
                                } else {
                                    control
                                };
                                if package.manifest.component.is_some() {
                                    let started = manager
                                        .begin_installation(
                                            &package,
                                            package.manifest.permissions.clone(),
                                            &control,
                                        )
                                        .and_then(|job| {
                                            BackgroundPreparation::start(
                                                package.manifest.id.clone(),
                                                job,
                                                control,
                                            )
                                        });
                                    match started {
                                        Ok(job) => {
                                            preparation = Some(job);
                                            bundled_install = candidate;
                                            // Starting preparation is not installation completion; retain the loading state.
                                            lifecycle = None;
                                            Ok(())
                                        }
                                        Err(error) => Err(error),
                                    }
                                } else {
                                    manager.install_with_control(
                                        &package,
                                        package.manifest.permissions.clone(),
                                        &control,
                                    )
                                }
                            })
                        }
                        Some(Work::Enable(id)) => manager.enable(&id),
                        Some(Work::Restart(id)) => manager.restart_plugin(&id),
                        Some(Work::SetTrust(trusted)) => manager.set_workspace_trust(trusted),
                        Some(Work::Disable(id)) => manager.disable(&id),
                        Some(Work::SetProjectEnabled(id, enabled)) => {
                            manager.set_project_enabled(&id, enabled)
                        }
                        Some(Work::Uninstall(id, delete)) => manager.uninstall(&id, delete),
                        Some(Work::ImportPreference {
                            plugin,
                            epoch,
                            owner,
                            workspace,
                            key,
                            data,
                        }) => {
                            let current = output
                                .lock()
                                .unwrap()
                                .instance_epochs
                                .get(&plugin)
                                .copied()
                                .unwrap_or(0);
                            let result = if current != epoch || manager.workspace() != workspace {
                                Err(api::Failure::new(
                                    api::ErrorCode::StaleRevision,
                                    "Preference import owner changed",
                                )
                                .into())
                            } else {
                                manager
                                    .import_preference(&plugin, key, data.clone())
                                    .map(|_| ())
                            };
                            let mut published = output.lock().unwrap();
                            // Missing UI acknowledgements cannot grow worker memory without bound;
                            // discarded receipts leave the legacy source intact for a later restart.
                            if published.preference_imports.len() >= 256 {
                                published.preference_imports.remove(0);
                            }
                            published.preference_imports.push(PreferenceImport {
                                owner,
                                workspace,
                                data,
                                succeeded: result.is_ok(),
                            });
                            result
                        }
                        Some(Work::Event(id, epoch, panel, event)) => {
                            let current = output
                                .lock()
                                .unwrap()
                                .instance_epochs
                                .get(&id)
                                .copied()
                                .unwrap_or(0);
                            if epoch == current {
                                let native_callback = matches!(
                                    event,
                                    api::Notification::Ui(_)
                                        | api::Notification::Preview { .. }
                                        | api::Notification::FilePreview { .. }
                                );
                                match manager.event(&id, panel, event) {
                                    Err(error)
                                        if native_callback
                                            && error
                                                .downcast_ref::<api::Failure>()
                                                .is_some_and(|failure| {
                                                    failure.code == api::ErrorCode::StaleRevision
                                                }) =>
                                    {
                                        // Only typed obsolete native input is benign. Guest faults and every other
                                        // rejection continue through the ordinary diagnostic/error path.
                                        output.lock().unwrap().logs.append(
                                            &id,
                                            plugin_runtime::logs::LogLevel::Info,
                                            "host.ui.stale",
                                            format!("{error:#}"),
                                        );
                                        Ok(())
                                    }
                                    other => other,
                                }
                            } else {
                                // Late native callbacks belong to an older surface, never to its replacement.
                                output.lock().unwrap().logs.append(
                                    &id,
                                    plugin_runtime::logs::LogLevel::Info,
                                    "host.ui.retired",
                                    t!("plugins.log_retired_callback").to_string(),
                                );
                                Ok(())
                            }
                        }
                        Some(Work::ImageInput {
                            plugin,
                            panel,
                            epoch,
                            document,
                            selection,
                            origin,
                            images,
                            reservation,
                        }) => {
                            let current = output
                                .lock()
                                .unwrap()
                                .instance_epochs
                                .get(&plugin)
                                .copied()
                                .unwrap_or(0);
                            let result = if epoch == current {
                                manager.offer_image_input(
                                    &plugin, &panel, document, selection, origin, images,
                                )
                            } else {
                                // Native preparation may finish after an upgrade; a replacement never owns these bytes.
                                Ok(())
                            };
                            drop(reservation);
                            result
                        }
                        Some(Work::Invoke {
                            plugin,
                            command,
                            arguments,
                        }) => manager.invoke_command(&plugin, &command, arguments),
                        None => Ok(()),
                    }
                };
                if preparation.is_none() {
                    // Failed preparation and resource-only installs release the immutable candidate promptly.
                    bundled_install = None;
                }
                match output.lock().unwrap().document_events.take_batch(64) {
                    Ok(changes) => {
                        for change in changes {
                            manager.document_changed(change);
                        }
                    }
                    Err(error) => manager.document_events_failed(error),
                }
                manager.poll();
                target_calls.poll(&manager, &output);
                configuration_calls.poll(&manager, &output);
                debug_requests.retain(|request, (method, pending)| {
                    let result = match pending.status() {
                        api::RequestUpdate::Accepted | api::RequestUpdate::Progress { .. } => {
                            return true;
                        }
                        api::RequestUpdate::Completed { result } => {
                            result.map_err(|error| error.message)
                        }
                        api::RequestUpdate::Cancelled { reason, .. } => Err(format!("{reason:?}")),
                    };
                    let answer = super::debug_answer(method, result);
                    output
                        .lock()
                        .unwrap()
                        .debug_answers
                        .push((*request, answer));
                    false
                });
                output.lock().unwrap().debug_observations = manager.debug_observations();
                let mut views = BTreeMap::new();
                // Publish only a real terminal reply or an actual query failure/timeout.
                run_polls.retain(|(config, request), completion| {
                    let status = match completion.status() {
                        api::RequestUpdate::Accepted | api::RequestUpdate::Progress { .. } => {
                            return true;
                        }
                        api::RequestUpdate::Completed { result: Ok(value) } => {
                            super::RunStatus::from_value(&value)
                        }
                        _ => super::RunStatus::Unknown,
                    };
                    let mut published = output.lock().unwrap();
                    published
                        .run_status
                        .push((config.clone(), *request, status));
                    published.configuration_revision += 1;
                    false
                });
                let mut processes = BTreeMap::new();
                run_locations.retain(|(session, request), completion| {
                    let result = match completion.status() {
                        api::RequestUpdate::Accepted | api::RequestUpdate::Progress { .. } => {
                            return true;
                        }
                        api::RequestUpdate::Completed { result } => {
                            result.map(|_| ()).map_err(|error| error.message)
                        }
                        api::RequestUpdate::Cancelled { reason, .. } => Err(format!("{reason:?}")),
                    };
                    output
                        .lock()
                        .unwrap()
                        .locate_results
                        .push((*session, *request, result));
                    false
                });
                let mut editor_requests = Vec::new();
                for (id, instance) in &mut manager.live {
                    editor_requests.extend(
                        instance
                            .take_editor_requests()
                            .into_iter()
                            .map(|request| (id.clone(), request)),
                    );
                    for (panel, scene) in &instance.views {
                        views.insert(format!("{id}/{panel}"), scene.clone());
                    }
                    processes.insert(id.clone(), instance.process_count());
                }
                if last_save.elapsed() > Duration::from_secs(3) {
                    if let Err(e) = manager.checkpoint() {
                        output.lock().unwrap().status = Some(OperationStatus {
                            plugin: None,
                            message: format!("保存插件状态失败：{e:#}"),
                        });
                    }
                    last_save = Instant::now();
                }
                // Vector parsing and rendering stay on this worker, outside the shared-state lock.
                let image_resources = manager.image_resources();
                let images = vectors.prepare_resources(&views, &image_resources);
                let configurations = manager
                    .installed
                    .keys()
                    .map(|id| {
                        (
                            id.clone(),
                            manager
                                .effective_settings(id)
                                .map_err(|error| format!("{error:#}")),
                        )
                    })
                    .collect::<BTreeMap<_, _>>();
                let language_services = manager.language_services();
                let structure_providers = manager.structure_providers();
                for service in language_services
                    .values()
                    .filter_map(|service| service.as_ref().ok())
                {
                    *processes.entry(service.owner.clone()).or_default() += service.process_count();
                }
                let plugin_service_choices = manager.service_choices();
                let mut published = output.lock().unwrap();
                // Top-level Debug and the configuration page both follow the current actual registry.
                published.debug_availability = Some(manager.debug_availability());
                published.debug_abilities = manager.debug_service_abilities();
                published.debug_provider_abilities = manager.all_debug_abilities();
                if published.plugin_service_choices != plugin_service_choices {
                    published.plugin_service_choices = plugin_service_choices;
                    published.configuration_revision += 1;
                }
                let services_changed = language_services.len() != published.language_services.len()
                    || language_services.iter().any(|(key, value)| {
                        match (value, published.language_services.get(key)) {
                            (Ok(current), Some(Ok(old))) => !Arc::ptr_eq(current, old),
                            (Err(current), Some(Err(old))) => current != old,
                            _ => true,
                        }
                    });
                if services_changed {
                    for (key, service) in &language_services {
                        let changed = match (service, published.language_services.get(key)) {
                            (Ok(new), Some(Ok(old))) => !Arc::ptr_eq(new, old),
                            (Err(new), Some(Err(old))) => new != old,
                            _ => true,
                        };
                        if !changed {
                            continue;
                        }
                        // Preparation errors retain the manifest-owned key before a lease exists.
                        let plugin = key.split_once('/').map(|(plugin, _)| plugin).unwrap_or(key);
                        let (level, message) = match service {
                            Ok(_) => (
                                plugin_runtime::logs::LogLevel::Info,
                                t!("plugins.log_service_prepared").to_string(),
                            ),
                            Err(error) => (plugin_runtime::logs::LogLevel::Error, error.clone()),
                        };
                        published.logs.append(
                            plugin,
                            level,
                            &format!("language.prepare:{key}"),
                            message,
                        );
                    }
                    // Retired providers must not leave a previous version's ready badge behind.
                    let unchanged = published.language_services.iter().filter_map(|(key, old)| {
                        matches!((old, language_services.get(key)), (Ok(old), Some(Ok(new))) if Arc::ptr_eq(old,new)).then_some(key.clone())
                    }).collect::<std::collections::BTreeSet<_>>();
                    published
                        .service_states
                        .retain(|key, _| unchanged.contains(key));
                    published.language_services = language_services;
                    published.configuration_revision += 1;
                }
                if published.configurations != configurations {
                    published.configurations = configurations;
                    published.configuration_revision += 1;
                }
                // Incarnation changes invalidate outline work even when the provider's key is unchanged.
                let structures_changed = structure_providers.len()
                    != published.structure_providers.len()
                    || structure_providers.iter().any(|(key, value)| {
                        match (value, published.structure_providers.get(key)) {
                            (Ok(current), Some(Ok(old))) => !Arc::ptr_eq(current, old),
                            (Err(current), Some(Err(old))) => current != old,
                            _ => true,
                        }
                    });
                if structures_changed {
                    for (key, result) in &structure_providers {
                        if let Err(error) = result {
                            let plugin =
                                key.split_once('/').map_or(key.as_str(), |(owner, _)| owner);
                            published.logs.append(
                                plugin,
                                plugin_runtime::logs::LogLevel::Error,
                                &format!("language.structure.prepare:{key}"),
                                error.clone(),
                            );
                        }
                    }
                    published.structure_providers = structure_providers;
                    published.configuration_revision += 1;
                }
                // Host executions are published as views; the launch identity joins each answer to
                // the request that produced it, so another window's session is never adopted here.
                let host_executions = manager
                    .executions()
                    .into_iter()
                    .map(|session| {
                        let id = session.id();
                        let snapshot = session.snapshot();
                        let (config, request_id) =
                            run_requests.get(&id).cloned().unwrap_or_default();
                        HostRunSnapshot {
                            id,
                            config,
                            request_id,
                            plugin: snapshot.plugin,
                            state: snapshot.state,
                            provider_session: snapshot.provider_session,
                            failure: snapshot.failure.map(|failure| failure.message),
                        }
                    })
                    .collect::<Vec<_>>();
                if published.host_executions != host_executions {
                    published.host_executions = host_executions;
                    published.configuration_revision += 1;
                }
                published
                    .editor_requests
                    .retain(|(_, request)| !request.status().is_terminal());
                for request in editor_requests {
                    if published.editor_requests.len() < 256 {
                        published.editor_requests.push(request);
                    } else {
                        request.1.finish(Err(api::Failure::new(
                            api::ErrorCode::LimitExceeded,
                            "Editor publication queue is full",
                        )));
                    }
                }
                if lifecycle
                    .as_ref()
                    .is_some_and(|operation| operation.action == LifecycleAction::Install)
                {
                    if let Some(report) = &mut published.installation {
                        report.cancellable = false;
                        report.installed = result.is_ok();
                        report.message = match &result {
                            Err(error) => {
                                format!("安装失败：{error:#}\n可关闭此窗口后重新点击安装重试。")
                            }
                            Ok(())
                                if manager.installed.get(&report.id).is_some_and(|entry| {
                                    !entry.manifest.language_servers.is_empty()
                                }) =>
                            {
                                "插件已安装；等待语言服务选择与启动…".into()
                            }
                            Ok(()) => "插件安装完成。".into(),
                        };
                    }
                    published.install_control = None;
                    let failures = published
                        .language_services
                        .iter()
                        .filter_map(|(key, service)| {
                            service
                                .as_ref()
                                .err()
                                .map(|error| format!("{key}：准备失败：{error}"))
                        })
                        .collect::<Vec<_>>();
                    if let Some(report) = &mut published.installation {
                        for failure in failures
                            .iter()
                            .filter(|line| line.starts_with(&format!("{}/", report.id)))
                        {
                            report.message.push_str(&format!("\n{failure}"));
                        }
                    }
                }
                // Startup loading ends only after Manager::open has restored every enabled plugin.
                published.startup.clear();
                if let Err(e) = &result {
                    let message = if let Some(operation) = &lifecycle {
                        format!("{}失败：{e:#}", operation.action.label())
                    } else {
                        format!("{e:#}")
                    };
                    if let Some(plugin) = &status_plugin {
                        published.logs.append(
                            plugin,
                            plugin_runtime::logs::LogLevel::Error,
                            "host.operation",
                            message.clone(),
                        );
                    }
                    published.status = Some(OperationStatus {
                        plugin: status_plugin,
                        message,
                    });
                } else if let Some(operation) =
                    lifecycle.as_ref().filter(|_| status_plugin.is_some())
                {
                    published.logs.append(
                        &operation.id,
                        plugin_runtime::logs::LogLevel::Info,
                        &format!("host.lifecycle.{:?}", operation.action),
                        t!("plugins.log_operation_complete").to_string(),
                    );
                }
                if lifecycle.is_some() {
                    published.progress = None;
                }
                // Recovery is a new incarnation even when installation returns Err. Every native surface must
                // resend its size/document and reject callbacks captured by the retired owner.
                let next_instances = manager
                    .live
                    .keys()
                    .filter_map(|id| {
                        manager
                            .instance_id(id)
                            .map(|identity| (id.clone(), identity.to_owned()))
                    })
                    .collect::<BTreeMap<_, _>>();
                for (id, identity) in &next_instances {
                    if instance_ids.get(id) != Some(identity) {
                        retire_publication(&mut published, id);
                    }
                }
                for id in instance_ids
                    .keys()
                    .filter(|id| !next_instances.contains_key(*id))
                {
                    retire_publication(&mut published, id);
                }
                instance_ids = next_instances;
                published.publish_entries(manager.published_entries());
                published.ready = true;
                published.diagnostics = manager
                    .installed
                    .keys()
                    .map(|id| (id.clone(), manager.diagnostics(id)))
                    .collect();
                published.views = views;
                published.images = images;
                published.processes = processes;
                published.generation += 1;
            }
            // Manager drop atomically saves plugin snapshots and closes owned process trees.
        });
        Self {
            tx,
            state,
            trusted: trust,
            image_offers: Default::default(),
            #[cfg(test)]
            recorded: Mutex::new(mpsc::channel().1),
            #[cfg(test)]
            run_queries: Default::default(),
        }
    }
}

/// Native callbacks retain their original owner; replacement advances the publication epoch.
fn retire_publication(published: &mut Published, id: &str) {
    *published.instance_epochs.entry(id.to_owned()).or_default() += 1;
}
