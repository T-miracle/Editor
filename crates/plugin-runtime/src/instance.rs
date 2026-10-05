//! Component isolation, capability enforcement and bounded guest calls.
use super::process::Processes;
use plugin_protocol::*;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};
use wasmtime::{
    Engine, Store,
    component::{Component, HasSelf, Linker},
};
use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxView, WasiView};
mod capability_calls;
mod document_events;
mod editor_requests;
mod file_discovery;
mod image_inputs;
mod plugin_services;
mod process_calls;
mod resource_roots;
mod settings;
mod stdio;
use resource_roots::ResourceRoots;
wasmtime::component::bindgen!({path:"../plugin-protocol/wit",world:"plugin",require_store_data_send:true});

struct State {
    plugin_services: plugin_services::Services,
    /// Every instance has one negotiated capability transport; there is no legacy fallback.
    api: api::Negotiated,
    /// Immutable metadata never becomes a preopened directory or guest-controlled file root.
    host_resources: crate::HostResources,
    native_diagnostics: crate::faults::NativeDiagnostics,
    /// Native discovery shares the enclosing WASM call deadline instead of refreshing it per request.
    call_deadline: std::time::Instant,
    roots: ResourceRoots,
    /// Slots and pending completions are owned by this exact WASM instance.
    editor_requests: std::collections::BTreeMap<u64, crate::editor_requests::PendingRequest>,
    /// Native inputs and their slots share this incarnation; pending writers retain their own immutable payload.
    image_inputs: std::collections::BTreeMap<u64, image_inputs::Input>,
    declared_panels: BTreeSet<String>,
    /// Editor toolbar authority is confined to this package's declared workspace preview surfaces.
    declared_editor_panels: BTreeSet<String>,
    /// Auxiliary tools never gain authority to publish the whole file layout.
    declared_layout_panels: BTreeSet<String>,
    subscriptions: std::collections::BTreeMap<u64, crate::document_events::Subscription>,
    wasi: WasiCtx,
    table: ResourceTable,
    limits: crate::faults::MemoryBudget,
    permissions: BTreeSet<String>,
    processes: Processes,
    /// Service definitions and process handles cannot be retargeted by guest request fields.
    services: std::collections::BTreeMap<String, plugin_protocol::process::Service>,
    process_handles: std::collections::BTreeMap<u64, (api::ResourceHandle, String)>,
    /// Each native process separately pins its immutable runtime files until it exits or is retired.
    process_dependencies: std::collections::BTreeMap<u64, Vec<std::sync::Arc<std::fs::File>>>,
    workspace: PathBuf,
    data: PathBuf,
    assets: PathBuf,
    active: bool,
    /// A bounded discovery invocation can read granted resources without launching native work.
    language_hook: bool,
    /// Read-only discovery can release only the file roots allocated by its own hook.
    language_hook_checkpoint: Option<u64>,
    /// Migration grants access exclusively to a transaction's isolated private-data copy.
    migrating: bool,
    /// Initialization writes commit only with the version switch; failed activation cannot edit old settings.
    staged_writes: Option<std::collections::BTreeMap<PathBuf, Vec<u8>>>,
}
impl WasiView for State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}
impl editor::plugin::host::Host for State {
    /// Deny both ungranted calls and side effects during update preparation.
    fn request(&mut self, payload: String) -> Result<String, String> {
        self.capability_request(&payload)
    }
}
/// Reject traversal, absolute/device paths and symlink escapes before any host file access.
pub(crate) fn safe_path(root: &Path, relative: &str, existing: bool) -> anyhow::Result<PathBuf> {
    anyhow::ensure!(
        !relative.is_empty()
            && !relative.contains(['\\', ':'])
            && Path::new(relative)
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_))),
        "Unsafe relative path"
    );
    let root = root.canonicalize()?;
    let path = root.join(relative);
    let checked = if existing || path.exists() {
        path.canonicalize()?
    } else {
        let parent = path.parent().unwrap().canonicalize()?;
        parent.join(path.file_name().unwrap())
    };
    anyhow::ensure!(checked.starts_with(&root), "Path escapes capability root");
    Ok(checked)
}
#[cfg(test)]
mod tests {
    use super::*;
    /// File capabilities stay under their canonical root, including absent write destinations.
    #[test]
    fn file_capability_cannot_escape_its_root() {
        let root = tempfile::tempdir().unwrap();
        for path in [
            "../outside",
            "/outside",
            "C:/outside",
            "a/../../outside",
            "a\\outside",
        ] {
            assert!(safe_path(root.path(), path, false).is_err());
        }
        assert!(safe_path(root.path(), "settings.json", false).is_ok());
    }
}

/// One isolated plugin has its own Store, WASI table, resource handles and fuel budget.
pub struct Instance {
    pub(crate) diagnostics: Vec<crate::faults::Diagnostic>,
    /// UI preview publication is tied to the latest authorized input for each declared surface.
    pub(crate) preview_sources: std::collections::BTreeMap<String, Option<api::DocumentVersion>>,
    /// File authority is independent of native text revisions and revoked on file/provider changes.
    pub(crate) file_sources: std::collections::BTreeMap<String, Option<api::FileContext>>,
    /// Configuration belongs to the same owner as its runtime resources, not the currently selected workspace.
    pub(crate) configuration: plugin_protocol::settings::Effective,
    /// Host invocation IDs cannot be confused with stale completions after another call.
    next_call: u64,
    store: Store<State>,
    bindings: Plugin,
    /// Validated documents keyed by their declared panel; publication is atomic across one reply.
    pub views: std::collections::BTreeMap<String, std::sync::Arc<ui::Document>>,
    panels: std::collections::BTreeSet<String>,
    pub error: Option<String>,
}
impl Instance {
    /// The identity changes on recovery as well as replacement; old host callbacks must not follow it.
    pub(crate) fn instance_id(&self) -> Option<&str> {
        self.store.data().active.then_some(
            self.store
                .data()
                .plugin_services
                .principal
                .instance
                .as_str(),
        )
    }
    pub fn engine() -> anyhow::Result<Engine> {
        let mut config = wasmtime::Config::new();
        config
            .wasm_component_model(true)
            .consume_fuel(true)
            .epoch_interruption(true);
        let engine = Engine::new(&config)?;
        let weak = engine.weak();
        // A weak engine reference lets the shared watchdog terminate after the manager is dropped.
        std::thread::Builder::new()
            .name("plugin-epoch-clock".into())
            .spawn(move || {
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(
                        crate::faults::EPOCH_TICK_MS,
                    ));
                    let Some(engine) = weak.upgrade() else { break };
                    engine.increment_epoch();
                }
            })?;
        Ok(engine)
    }
    /// Embedders without host SDK metadata retain the ordinary component preparation entry point.
    pub fn prepare(
        engine: &Engine,
        bytes: &[u8],
        manifest: &Manifest,
        grants: &BTreeSet<String>,
        environment: Environment,
        data: PathBuf,
        assets: PathBuf,
        snapshot: Option<Snapshot>,
    ) -> anyhow::Result<Self> {
        Self::prepare_with_resources(
            engine,
            bytes,
            manifest,
            grants,
            environment,
            data,
            assets,
            snapshot,
            Default::default(),
        )
    }
    /// Every candidate receives the same immutable host inputs before any lifecycle callback.
    pub(crate) fn prepare_with_resources(
        engine: &Engine,
        bytes: &[u8],
        manifest: &Manifest,
        grants: &BTreeSet<String>,
        mut environment: Environment,
        data: PathBuf,
        assets: PathBuf,
        snapshot: Option<Snapshot>,
        host_resources: crate::HostResources,
    ) -> anyhow::Result<Self> {
        let api = super::capabilities::negotiate(manifest)?;
        manifest
            .plugin_services
            .validate()
            .map_err(anyhow::Error::msg)?;
        let application = manifest.scope == api::InstanceScope::Application;
        if application {
            environment.workspace.clear();
        }
        anyhow::ensure!(
            manifest.permissions.is_subset(grants),
            "Plugin needs additional permission consent"
        );
        std::fs::create_dir_all(&data)?;
        let component = Component::new(engine, bytes).inspect_err(|error| {
            host_resources.logs.append(
                &manifest.id,
                crate::LogLevel::Error,
                "wasm/compile",
                format!("{error:#}"),
            );
        })?;
        let mut linker = Linker::<State>::new(engine);
        wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;
        Plugin::add_to_linker::<_, HasSelf<_>>(&mut linker, |state| state)?;
        let stdout = stdio::capture(
            host_resources.logs.clone(),
            &manifest.id,
            crate::LogLevel::Info,
            "wasi/stdout",
        )?;
        let stderr = stdio::capture(
            host_resources.logs.clone(),
            &manifest.id,
            crate::LogLevel::Warning,
            "wasi/stderr",
        )?;
        let native_diagnostics =
            crate::faults::NativeDiagnostics::with_logs(host_resources.logs.clone());
        let mut wasi = WasiCtx::builder();
        wasi.stdout(stdout).stderr(stderr);
        // No preopened directories, environment inheritance or network access is granted to WASI.
        let mut state = State {
            plugin_services: Default::default(),
            api,
            host_resources,
            native_diagnostics,
            call_deadline: std::time::Instant::now(),
            roots: ResourceRoots::new(&environment.workspace, application, manifest.storage_limit),
            editor_requests: Default::default(),
            image_inputs: Default::default(),
            subscriptions: Default::default(),
            declared_panels: manifest
                .panels
                .iter()
                .map(|panel| panel.id.clone())
                .collect(),
            declared_editor_panels: manifest
                .panels
                .iter()
                .filter(|panel| panel.position == "editor")
                .map(|panel| panel.id.clone())
                .collect(),
            declared_layout_panels: manifest
                .panels
                .iter()
                .filter(|panel| panel.position == "editor" && !panel.auxiliary)
                .map(|panel| panel.id.clone())
                .collect(),
            wasi: wasi.build(),
            table: ResourceTable::new(),
            limits: Default::default(),
            permissions: manifest.permissions.clone(),
            processes: Processes::default(),
            services: manifest.services.clone(),
            process_handles: Default::default(),
            process_dependencies: Default::default(),
            workspace: PathBuf::from(&environment.workspace),
            data,
            assets,
            active: false,
            language_hook: false,
            language_hook_checkpoint: None,
            migrating: false,
            staged_writes: Some(Default::default()),
        };
        state.plugin_services.principal =
            state.roots.principal(&manifest.id, &manifest.permissions);
        state.plugin_services.declarations = manifest.plugin_services.clone();
        let mut store = Store::new(engine, state);
        store.limiter(|s| &mut s.limits);
        store.set_fuel(100_000_000)?;
        store.set_epoch_deadline(crate::faults::CALL_DEADLINE_MS / crate::faults::EPOCH_TICK_MS);
        let bindings =
            Plugin::instantiate(&mut store, &component, &linker).inspect_err(|error| {
                store.data().host_resources.logs.append(
                    &manifest.id,
                    crate::LogLevel::Error,
                    "wasm/instantiate",
                    format!("{error:#}"),
                );
            })?;
        let mut instance = Self {
            diagnostics: Vec::new(),
            preview_sources: Default::default(),
            file_sources: Default::default(),
            configuration: Default::default(),
            next_call: 1,
            store,
            bindings,
            views: Default::default(),
            panels: manifest.panels.iter().map(|p| p.id.clone()).collect(),
            error: None,
        };
        instance.prepare_state(environment, snapshot)?;
        Ok(instance)
    }
    /// Immutable negotiated authority lets manager ingress reject unavailable optional interfaces.
    pub(crate) fn negotiated(&self) -> &api::Negotiated {
        &self.store.data().api
    }

    /// Every call gets a finite instruction budget; a trap cannot unwind through the host.
    pub fn call(&mut self, message: api::Input) -> anyhow::Result<api::Output> {
        anyhow::ensure!(
            !self.store.data().roots.retired,
            "Plugin is paused; restart this instance"
        );
        let operation = crate::faults::operation(&message);
        let result = self.call_inner(message);
        if let Err(error) = &result {
            let principal = &self.store.data().plugin_services.principal;
            let budget = error.downcast_ref::<wasmtime::Trap>().is_some_and(|trap| {
                matches!(trap, wasmtime::Trap::OutOfFuel | wasmtime::Trap::Interrupt)
            });
            let message: String = format!(
                "{}: {error:#}",
                if budget {
                    "WASM execution budget exceeded"
                } else {
                    "WASM operation failed"
                }
            )
            .chars()
            .take(4096)
            .collect();
            self.error = Some(format!(
                "{} [{}] {operation}: {message}",
                principal.plugin, principal.scope
            ));
            // Log at the originating call, not when a later UI selection happens to publish diagnostics.
            self.store.data().host_resources.logs.append(
                &principal.plugin,
                crate::LogLevel::Error,
                &format!("wasm/{operation}"),
                format!("scope={} {message}", principal.scope),
            );
            self.diagnostics.push(crate::faults::Diagnostic::new(
                principal.plugin.clone(),
                principal.scope.clone(),
                operation,
                message,
            ));
            if self.diagnostics.len() > 32 {
                self.diagnostics.remove(0);
            }
            if error.downcast_ref::<api::Failure>().is_none() {
                self.stop();
            }
        }
        result.map_err(|error| {
            let principal = &self.store.data().plugin_services.principal;
            let operation = self
                .diagnostics
                .last()
                .map(|report| report.operation.as_str())
                .unwrap_or("call");
            error.context(format!(
                "plugin={} scope={} operation={operation}",
                principal.plugin, principal.scope
            ))
        })
    }
    /// Validation and publication share the same budgeted call; a failing result never partially publishes UI.
    fn call_inner(&mut self, mut message: api::Input) -> anyhow::Result<api::Output> {
        anyhow::ensure!(
            !self.store.data().roots.retired,
            "Instance has been retired"
        );
        // Application owners receive appearance updates without inheriting a selected workspace.
        if self.store.data().roots.application {
            match &mut message {
                api::Input::Prepare { environment, .. }
                | api::Input::Event {
                    event: api::Notification::Theme(environment),
                    ..
                } => environment.workspace.clear(),
                _ => {}
            }
        }
        // Snapshot serialization may traverse the full configured scrollback, while input and
        // paint events stay on the smaller interactive budget. Both calls remain fuel-bounded.
        let fuel = if matches!(message, api::Input::Snapshot) {
            crate::faults::SNAPSHOT_FUEL
        } else {
            crate::faults::CALL_FUEL
        };
        self.store.set_fuel(fuel)?;
        let timeout = if matches!(message, api::Input::Snapshot) {
            crate::faults::SNAPSHOT_DEADLINE_MS
        } else {
            crate::faults::CALL_DEADLINE_MS
        };
        self.store
            .set_epoch_deadline(timeout / crate::faults::EPOCH_TICK_MS);
        self.store.data_mut().call_deadline =
            std::time::Instant::now() + std::time::Duration::from_millis(timeout);
        let (payload, invocation_id) = self.encode_invocation(message)?;
        let result = self
            .bindings
            .call_dispatch(&mut self.store, &payload)?
            .map_err(anyhow::Error::msg)?;
        anyhow::ensure!(
            result.len() <= 32 * 1024 * 1024,
            "Plugin reply quota exceeded"
        );
        let reply = self.decode_completion(&result, invocation_id)?;
        let mut panels = std::collections::BTreeSet::new();
        for view in &reply.views {
            anyhow::ensure!(
                self.panels.contains(&view.panel),
                "Undeclared panel: {}",
                view.panel
            );
            anyhow::ensure!(
                panels.insert(&view.panel),
                "Duplicate panel in plugin reply"
            );
        }
        // Publish only after every document validates; an invalid peer never partly updates the UI.
        for view in &reply.views {
            self.views.insert(
                view.panel.clone(),
                std::sync::Arc::new(view.document.clone()),
            );
        }
        // Revoke against the newly published opt-in, before a second public event can use old slots.
        // Accepted writers pin their payload independently and still deliver the original save receipt.
        self.reconcile_image_inputs();
        self.error = None;
        Ok(reply)
    }
    pub fn activate(&mut self) -> anyhow::Result<()> {
        self.store.data_mut().active = true;
        self.call(api::Input::Activate)?;
        Ok(())
    }
    /// Attach before activation so native retirement reports survive this particular incarnation.
    pub(crate) fn connect_diagnostics(&mut self, diagnostics: crate::faults::NativeDiagnostics) {
        self.store.data_mut().native_diagnostics = diagnostics;
    }
    /// Preparation always supplies this instance's negotiated interfaces, including on rollback.
    pub(crate) fn prepare_state(
        &mut self,
        environment: Environment,
        snapshot: Option<Snapshot>,
    ) -> anyhow::Result<api::Output> {
        self.call(api::Input::Prepare {
            environment,
            snapshot,
            api: self.store.data().api.clone(),
        })
    }
    /// Native owners route typed notifications without a second event codec.
    pub fn notify(
        &mut self,
        panel: Option<String>,
        event: api::Notification,
    ) -> anyhow::Result<api::Output> {
        self.call(api::Input::Event { panel, event })
    }
    /// Apply buffered initialization writes while retaining originals until registry commit succeeds.
    pub(crate) fn commit_data(&mut self) -> anyhow::Result<Vec<(PathBuf, Option<Vec<u8>>)>> {
        let writes = self
            .store
            .data_mut()
            .staged_writes
            .take()
            .unwrap_or_default();
        let mut originals = vec![];
        let result = (|| {
            for (path, bytes) in writes {
                let old = match std::fs::read(&path) {
                    Ok(bytes) => Some(bytes),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                    Err(e) => return Err(e.into()),
                };
                originals.push((path.clone(), old));
                super::package::atomic_write(&path, &bytes)?;
            }
            Ok::<_, anyhow::Error>(())
        })();
        if let Err(error) = result {
            Self::rollback_data(&originals)?;
            return Err(error);
        }
        Ok(originals)
    }
    /// Restore only files touched by this transaction; unrelated plugin data stays intact.
    pub(crate) fn rollback_data(originals: &[(PathBuf, Option<Vec<u8>>)]) -> anyhow::Result<()> {
        for (path, bytes) in originals.iter().rev() {
            if let Some(bytes) = bytes {
                super::package::atomic_write(path, bytes)?;
            } else if path.exists() {
                std::fs::remove_file(path)?;
            }
        }
        Ok(())
    }
    /// Seal already-published work before the final snapshot, while retaining private-file access for serialization.
    pub(crate) fn quiesce(&mut self) {
        self.clear_image_inputs();
        self.store.data_mut().plugin_services.clear();
        self.store.data_mut().subscriptions.clear();
        for request in self.store.data_mut().editor_requests.values() {
            request.call.retire();
        }
        self.store.data_mut().editor_requests.clear();
    }
    pub fn stop(&mut self) {
        self.quiesce();
        self.preview_sources.clear();
        self.file_sources.clear();
        self.store.data_mut().roots.retire();
        self.views.clear();
        self.store.data_mut().processes.clear();
        self.store.data_mut().process_handles.clear();
        self.store.data_mut().process_dependencies.clear();
        self.store.data_mut().active = false;
    }
    pub fn process_count(&self) -> usize {
        self.store.data().processes.len()
    }
    /// Observable ownership count includes native views and file/process resources.
    pub fn resource_count(&self) -> usize {
        self.store.data().roots.len() + self.views.len() + self.process_count()
    }
    /// Rollback is an explicit host transition with fresh ownership, never revival of old handles.
    pub(crate) fn restart(
        &mut self,
        mut environment: Environment,
        snapshot: Option<Snapshot>,
    ) -> anyhow::Result<()> {
        if self.store.data().roots.application {
            environment.workspace.clear();
        }
        let roots = self.store.data().roots.renewed(&environment.workspace);
        self.store.data_mut().roots = roots;
        let principal = self.store.data().roots.principal(
            &self.store.data().plugin_services.principal.plugin,
            &self.store.data().permissions,
        );
        self.store.data_mut().plugin_services.principal = principal;
        // A restarted owner cannot revive authority captured by an earlier incarnation.
        self.store.data_mut().plugin_services.alive =
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        self.store.data_mut().staged_writes = Some(std::collections::BTreeMap::new());
        self.prepare_state(environment, snapshot)?;
        self.reapply_settings()?;
        self.activate()?;
        self.commit_data()?;
        Ok(())
    }
    pub fn process_ids(&self) -> Vec<u32> {
        self.store.data().processes.ids()
    }
    pub fn poll(&mut self) -> anyhow::Result<bool> {
        self.reconcile_image_inputs();
        self.retire_service_sources();
        let revoked = self.poll_service_revocations()?;
        self.poll_service_requests()?;
        self.poll_editor_requests()?;
        self.poll_document_events()?;
        let events = self.store.data_mut().poll_processes()?;
        let changed = revoked || !events.is_empty();
        for event in events {
            // Exit removes its slot, but the last callback still inherits the originating service authority.
            let context = if let api::Notification::Process { handle, .. } = &event {
                self.store
                    .data()
                    .plugin_services
                    .resources
                    .get(&handle.resource)
                    .map(|(_, context)| context.clone())
            } else {
                None
            };
            if self.store.data_mut().accept_process_event(&event) {
                let finished = if let api::Notification::Process {
                    handle,
                    update: process::Update::Exited { .. },
                } = &event
                {
                    Some(handle.resource)
                } else {
                    None
                };
                let result = self
                    .call_with_service_context(context, api::Input::Event { panel: None, event });
                if let Some(slot) = finished {
                    self.store
                        .data_mut()
                        .plugin_services
                        .resources
                        .remove(&slot);
                }
                result?;
            }
        }
        Ok(changed)
    }
    pub fn snapshot(&mut self) -> anyhow::Result<Snapshot> {
        self.call(api::Input::Snapshot)?
            .snapshot
            .ok_or_else(|| anyhow::anyhow!("Plugin did not return its declared state"))
    }
}
