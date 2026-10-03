//! The production actor keeps dispatching old instances while a separate thread prepares a candidate.
use super::preparation::BackgroundPreparation;
use super::*;
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
        std::thread::spawn(move || {
            // Prepare the public SDK off the UI thread. A failure is delivered only to guests that request it.
            let resources = plugin_runtime::HostResources {
                sdk: Some(crate::sdk_export::descriptor().map_err(|error| format!("{error:#}"))),
            };
            let mut manager =
                match Manager::open_with_resources(root, environment, trusted, resources) {
                    Ok(m) => m,
                    Err(e) => {
                        let mut published = output.lock().unwrap();
                        published.startup.clear();
                        published.status = Some(format!("{e:#}"));
                        return;
                    }
                };
            let mut last_save = Instant::now();
            let mut vectors = super::super::images::VectorRenderer::default();
            let mut preparation: Option<BackgroundPreparation> = None;
            let mut deferred = VecDeque::new();
            let mut instance_ids = BTreeMap::<String, String>::new();
            loop {
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
                            | Work::SetServiceProvider { .. }
                            | Work::Enable(_)
                            | Work::Restart(_)
                            | Work::Disable(_)
                            | Work::SetProjectEnabled(_, _)
                            | Work::SetTrust(true)
                            | Work::Uninstall(_, _)
                            | Work::Install(_),
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
                    prepared.and_then(|prepared| manager.commit_installation(prepared, &control))
                } else {
                    match work {
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
                        Some(Work::Shutdown(ack)) => {
                            drop(manager);
                            if let Some(ack) = ack {
                                let _ = ack.send(());
                            }
                            return;
                        }
                        Some(Work::Inspect(path)) => Package::read(&path)
                            .map(|package| output.lock().unwrap().pending = Some(package)),
                        Some(Work::Install(package)) => {
                            let control = output
                                .lock()
                                .unwrap()
                                .install_control
                                .clone()
                                .unwrap_or_default();
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
                        }
                        Some(Work::Enable(id)) => manager.enable(&id),
                        Some(Work::Restart(id)) => manager.restart_plugin(&id),
                        Some(Work::SetTrust(trusted)) => manager.set_workspace_trust(trusted),
                        Some(Work::Disable(id)) => manager.disable(&id),
                        Some(Work::SetProjectEnabled(id, enabled)) => {
                            manager.set_project_enabled(&id, enabled)
                        }
                        Some(Work::Uninstall(id, delete)) => manager.uninstall(&id, delete),
                        Some(Work::Event(id, epoch, panel, event)) => {
                            let current = output
                                .lock()
                                .unwrap()
                                .instance_epochs
                                .get(&id)
                                .copied()
                                .unwrap_or(0);
                            if epoch == current {
                                manager.event(&id, panel, event)
                            } else {
                                // Late native callbacks belong to an older surface, never to its replacement.
                                Ok(())
                            }
                        }
                        Some(Work::Invoke {
                            plugin,
                            command,
                            arguments,
                        }) => manager.invoke_command(&plugin, &command, arguments),
                        None => Ok(()),
                    }
                };
                match output.lock().unwrap().document_events.take_batch(64) {
                    Ok(changes) => {
                        for change in changes {
                            manager.document_changed(change);
                        }
                    }
                    Err(error) => manager.document_events_failed(error),
                }
                manager.poll();
                let mut views = BTreeMap::new();
                let mut processes = BTreeMap::new();
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
                        output.lock().unwrap().status = Some(format!("保存插件状态失败：{e:#}"));
                    }
                    last_save = Instant::now();
                }
                // Vector parsing and rendering stay on this worker, outside the shared-state lock.
                let images = vectors.prepare(&views);
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
                for service in language_services
                    .values()
                    .filter_map(|service| service.as_ref().ok())
                {
                    *processes.entry(service.owner.clone()).or_default() += service.process_count();
                }
                let plugin_service_choices = manager.service_choices();
                let mut published = output.lock().unwrap();
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
                if let Err(e) = result {
                    published.status = Some(if let Some(operation) = &lifecycle {
                        format!("{}失败：{e:#}", operation.action.label())
                    } else {
                        format!("{e:#}")
                    });
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
                published.entries = manager.published_entries();
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
            trusted: std::sync::atomic::AtomicBool::new(trusted),
            #[cfg(test)]
            recorded: Mutex::new(mpsc::channel().1),
        }
    }
}

/// Native callbacks retain their original owner; replacement advances the publication epoch.
fn retire_publication(published: &mut Published, id: &str) {
    *published.instance_epochs.entry(id.to_owned()).or_default() += 1;
}
