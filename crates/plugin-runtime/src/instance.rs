//! Component isolation, capability enforcement and bounded guest calls.
use super::process::Processes;
use plugin_protocol::*;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};
use wasmtime::{
    Engine, Store, StoreLimits, StoreLimitsBuilder,
    component::{Component, HasSelf, Linker},
};
use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxView, WasiView};
mod capability_calls;
wasmtime::component::bindgen!({path:"../plugin-protocol/wit",world:"plugin",require_store_data_send:true});

struct State {
    /// None selects the temporary legacy transport, never a fallback for a new guest.
    api: Option<api::Negotiated>,
    wasi: WasiCtx,
    table: ResourceTable,
    limits: StoreLimits,
    permissions: BTreeSet<String>,
    processes: Processes,
    workspace: PathBuf,
    data: PathBuf,
    assets: PathBuf,
    active: bool,
    effects: Vec<Request>,
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
        if self.api.is_some() {
            return self.capability_request(&payload);
        }
        self.request_checked(&payload).map_err(|e| format!("{e:#}"))
    }
}
impl State {
    fn request_checked(&mut self, payload: &str) -> anyhow::Result<String> {
        anyhow::ensure!(payload.len() <= 2 * 1024 * 1024, "Host request too large");
        let request: Request = serde_json::from_str(payload)?;
        anyhow::ensure!(
            request.permission() == "assets" || self.permissions.contains(request.permission()),
            "Permission denied: {}",
            request.permission()
        );
        anyhow::ensure!(
            self.active || matches!(request, Request::ReadAsset { .. }),
            "Host calls are unavailable during plugin preparation"
        );
        let value = match request {
            Request::ReadAsset { path } => {
                let path = safe_path(&self.assets, &path, true)?;
                anyhow::ensure!(
                    path.metadata()?.len() <= 4 * 1024 * 1024,
                    "Asset read quota exceeded"
                );
                serde_json::json!(std::fs::read(path)?)
            }
            Request::Spawn {
                program,
                args,
                cwd,
                columns,
                rows,
            } => serde_json::json!(self.processes.spawn(program, args, cwd, columns, rows)?),
            Request::Write { handle, bytes } => {
                self.processes.write(handle, &bytes)?;
                serde_json::Value::Null
            }
            Request::Resize {
                handle,
                columns,
                rows,
            } => {
                self.processes.resize(handle, columns, rows)?;
                serde_json::Value::Null
            }
            Request::Close { handle } => {
                self.processes.close(handle)?;
                serde_json::Value::Null
            }
            Request::ReadWorkspace { path } => {
                serde_json::json!(read_bounded(&safe_path(&self.workspace, &path, true)?)?)
            }
            Request::ReadData { path } => {
                let path = safe_path(&self.data, &path, false)?;
                if let Some(bytes) = self
                    .staged_writes
                    .as_ref()
                    .and_then(|writes| writes.get(&path))
                {
                    serde_json::json!(String::from_utf8(bytes.clone())?)
                } else {
                    serde_json::json!(read_bounded(&path)?)
                }
            }
            Request::WriteData { path, text } => {
                anyhow::ensure!(text.len() <= 1024 * 1024, "Data file quota exceeded");
                let path = safe_path(&self.data, &path, false)?;
                let used = std::fs::read_dir(&self.data)?
                    .filter_map(Result::ok)
                    .filter_map(|e| e.metadata().ok())
                    .map(|m| m.len())
                    .sum::<u64>();
                let replaced = path.metadata().map(|m| m.len()).unwrap_or(0);
                anyhow::ensure!(
                    used.saturating_sub(replaced) + text.len() as u64 <= 64 * 1024 * 1024,
                    "Plugin storage quota exceeded"
                );
                if let Some(writes) = &mut self.staged_writes {
                    anyhow::ensure!(
                        writes.len() < 64
                            && writes.values().map(Vec::len).sum::<usize>() + text.len()
                                <= 4 * 1024 * 1024,
                        "Initialization write quota exceeded"
                    );
                    writes.insert(path, text.into_bytes());
                } else {
                    super::package::atomic_write(&path, text.as_bytes())?;
                }
                serde_json::Value::Null
            }
            request => {
                anyhow::ensure!(self.effects.len() < 64, "UI request quota exceeded");
                self.effects.push(request);
                serde_json::Value::Null
            }
        };
        Ok(serde_json::to_string(&value)?)
    }
}

/// Untrusted geometry must not reach native layout/text code with NaNs or unbounded sizes.
fn validate_scene(scene: &Scene) -> anyhow::Result<()> {
    if let Some(controls) = &scene.controls {
        anyhow::ensure!(
            scene.ui.is_none() && scene.widgets.is_empty(),
            "Canvas controls cannot mix with Document or legacy widgets"
        );
        controls.validate().map_err(anyhow::Error::msg)?;
    }
    if let Some(document) = &scene.ui {
        anyhow::ensure!(
            scene.paint.is_empty()
                && scene.widgets.is_empty()
                && scene.scroll.is_none()
                && scene.column_resize_regions.is_empty(),
            "A scene must choose either native UI or canvas"
        );
        return document.validate().map_err(anyhow::Error::msg);
    }
    let coordinate = |v: f32| v.is_finite() && v.abs() <= 1_000_000.;
    let rect = |r: &Rect| {
        coordinate(r.x)
            && coordinate(r.y)
            && coordinate(r.w)
            && coordinate(r.h)
            && r.w >= 0.
            && r.h >= 0.
    };
    anyhow::ensure!(
        scene.font.len() <= 256 && (1. ..=128.).contains(&scene.font_size) && rect(&scene.cursor),
        "Invalid scene metrics"
    );
    for paint in &scene.paint {
        anyhow::ensure!(
            match paint {
                Paint::Fill { rect: r, .. } => rect(r),
                Paint::Svg {
                    rect: r,
                    clip,
                    source,
                } => rect(r) && rect(clip) && !source.is_empty() && source.len() <= 1024 * 1024,
                Paint::Text {
                    x,
                    y,
                    text,
                    size,
                    font,
                    ..
                } =>
                    coordinate(*x)
                        && coordinate(*y)
                        && (1. ..=128.).contains(size)
                        && text.len() <= 65536
                        && font
                            .as_ref()
                            .is_none_or(|family| !family.trim().is_empty() && family.len() <= 256),
            },
            "Invalid paint operation"
        );
    }
    // Vector source and raster work have their own quota, independent of cheap rectangle drawing.
    let vectors = scene
        .paint
        .iter()
        .filter_map(|paint| {
            if let Paint::Svg { source, .. } = paint {
                Some(source.len())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    anyhow::ensure!(
        vectors.len() <= 16 && vectors.iter().sum::<usize>() <= 2 * 1024 * 1024,
        "Vector scene quota exceeded"
    );
    anyhow::ensure!(
        scene.widgets.iter().all(|w| {
            rect(&w.rect)
                && w.id.len() <= 256
                && w.label.len() <= 65536
                && w.style
                    .font
                    .family
                    .as_ref()
                    .is_none_or(|family| !family.trim().is_empty() && family.len() <= 256)
                && w.style
                    .font
                    .size_px
                    .is_none_or(|size| (1. ..=128.).contains(&size))
        }),
        "Invalid widget"
    );
    // Cursor hit areas come from untrusted guests and must stay bounded like widgets.
    anyhow::ensure!(
        scene.column_resize_regions.len() <= 16 && scene.column_resize_regions.iter().all(rect),
        "Invalid cursor region"
    );
    if let Some(scroll) = &scene.scroll {
        anyhow::ensure!(
            rect(&scroll.rect)
                && scroll.content.is_finite()
                && scroll.content >= 0.
                && scroll.content <= 1_000_000_000.
                && scroll.offset.is_finite()
                && scroll.offset >= 0.,
            "Invalid scroll range"
        );
    }
    Ok(())
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
fn read_bounded(path: &Path) -> anyhow::Result<String> {
    anyhow::ensure!(path.metadata()?.len() <= 1024 * 1024, "Read quota exceeded");
    Ok(std::fs::read_to_string(path)?)
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

    /// Host file requests must respect both consent and each plugin's own directory.
    #[test]
    fn file_requests_require_grants_and_cannot_reach_outside_plugin_roots() {
        let root = tempfile::tempdir().unwrap();
        let assets = root.path().join("assets");
        let data = root.path().join("data");
        let workspace = root.path().join("workspace");
        for directory in [&assets, &data, &workspace] {
            std::fs::create_dir(directory).unwrap();
            std::fs::write(directory.join("allowed.txt"), "allowed").unwrap();
        }
        let outside = root.path().join("outside.txt");
        std::fs::write(&outside, "private").unwrap();
        let mut state = State {
            api: None,
            wasi: WasiCtx::builder().build(),
            table: ResourceTable::new(),
            limits: StoreLimitsBuilder::new().build(),
            permissions: BTreeSet::new(),
            processes: Processes::default(),
            workspace,
            data,
            assets,
            active: true,
            effects: vec![],
            staged_writes: None,
        };
        let send = |state: &mut State, request: Request| {
            state.request_checked(&serde_json::to_string(&request).unwrap())
        };
        assert!(
            send(
                &mut state,
                Request::ReadAsset {
                    path: "allowed.txt".into()
                }
            )
            .is_ok()
        );
        assert!(
            send(
                &mut state,
                Request::ReadData {
                    path: "allowed.txt".into()
                }
            )
            .unwrap_err()
            .to_string()
            .contains("Permission denied")
        );
        assert!(
            send(
                &mut state,
                Request::ReadWorkspace {
                    path: "allowed.txt".into()
                }
            )
            .unwrap_err()
            .to_string()
            .contains("Permission denied")
        );
        assert!(
            send(
                &mut state,
                Request::WriteData {
                    path: "allowed.txt".into(),
                    text: "forbidden".into(),
                }
            )
            .unwrap_err()
            .to_string()
            .contains("Permission denied")
        );
        state.permissions = ["storage".into(), "workspace.read".into()].into();
        assert!(
            send(
                &mut state,
                Request::ReadData {
                    path: "allowed.txt".into()
                }
            )
            .is_ok()
        );
        assert!(
            send(
                &mut state,
                Request::ReadWorkspace {
                    path: "allowed.txt".into()
                }
            )
            .is_ok()
        );
        for path in [
            "../outside.txt".to_owned(),
            "../data/allowed.txt".to_owned(),
            "../workspace/allowed.txt".to_owned(),
            outside.to_string_lossy().into_owned(),
            "\\\\?\\C:\\outside.txt".to_owned(),
            "allowed.txt:stream".to_owned(),
        ] {
            assert!(send(&mut state, Request::ReadAsset { path: path.clone() }).is_err());
            assert!(send(&mut state, Request::ReadData { path: path.clone() }).is_err());
            assert!(send(&mut state, Request::ReadWorkspace { path: path.clone() }).is_err());
            assert!(
                send(
                    &mut state,
                    Request::WriteData {
                        path,
                        text: "forbidden".into(),
                    }
                )
                .is_err()
            );
        }
        assert_eq!(std::fs::read_to_string(outside).unwrap(), "private");
        assert_eq!(
            std::fs::read_to_string(state.data.join("allowed.txt")).unwrap(),
            "allowed"
        );
    }
    /// Bad plugin coordinates must be rejected before reaching the native renderer.
    #[test]
    fn rejects_invalid_scene_geometry() {
        let mut scene = Scene {
            font: "mono".into(),
            font_size: 14.,
            ..Scene::default()
        };
        assert!(validate_scene(&scene).is_ok());
        scene.cursor.x = f32::NAN;
        assert!(validate_scene(&scene).is_err());
    }
    /// A native tree needs no canvas font metrics, but cannot carry a second rendering model.
    #[test]
    fn native_scenes_validate_the_tree_and_reject_mixed_renderers() {
        let mut scene = Scene {
            ui: Some(ui::Document::new(ui::Node::button("run", "Run"))),
            ..Default::default()
        };
        assert!(validate_scene(&scene).is_ok());
        scene.paint.push(Paint::Fill {
            rect: Rect::default(),
            color: 0,
            extend_to_bottom: false,
        });
        assert!(validate_scene(&scene).is_err());
        scene.paint.clear();
        scene.ui.as_mut().unwrap().version = 999;
        assert!(validate_scene(&scene).is_err());
    }
    /// Activation-time writes remain invisible on disk until the enclosing update commits.
    #[test]
    fn failed_initialization_cannot_modify_existing_settings() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("settings.json"), "old").unwrap();
        let mut state = State {
            api: None,
            wasi: WasiCtx::builder().build(),
            table: ResourceTable::new(),
            limits: StoreLimitsBuilder::new().build(),
            permissions: ["storage".into()].into(),
            processes: Processes::default(),
            workspace: root.path().into(),
            data: root.path().into(),
            assets: root.path().into(),
            active: true,
            effects: vec![],
            staged_writes: Some(Default::default()),
        };
        let request = serde_json::to_string(&Request::WriteData {
            path: "settings.json".into(),
            text: "new".into(),
        })
        .unwrap();
        state.request_checked(&request).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.path().join("settings.json")).unwrap(),
            "old"
        );
        drop(state);
        assert_eq!(
            std::fs::read_to_string(root.path().join("settings.json")).unwrap(),
            "old"
        );
    }
}

/// One isolated plugin has its own Store, WASI table, resource handles and fuel budget.
pub struct Instance {
    /// Host invocation IDs cannot be confused with stale completions after another call.
    next_call: u64,
    protocol: u32,
    store: Store<State>,
    bindings: Plugin,
    pub scene: Option<std::sync::Arc<Scene>>,
    pub scenes: std::collections::BTreeMap<String, std::sync::Arc<Scene>>,
    panels: std::collections::BTreeSet<String>,
    pub error: Option<String>,
}
impl Instance {
    pub fn engine() -> anyhow::Result<Engine> {
        let mut config = wasmtime::Config::new();
        config.wasm_component_model(true).consume_fuel(true);
        Ok(Engine::new(&config)?)
    }
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
        let api = super::capabilities::negotiate(manifest)?;
        anyhow::ensure!(
            manifest.permissions.is_subset(grants),
            "Plugin needs additional permission consent"
        );
        std::fs::create_dir_all(&data)?;
        let component = Component::new(engine, bytes)?;
        let mut linker = Linker::<State>::new(engine);
        wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;
        Plugin::add_to_linker::<_, HasSelf<_>>(&mut linker, |state| state)?;
        // No preopened directories, environment inheritance or network access is granted to WASI.
        let state = State {
            api,
            wasi: WasiCtx::builder().build(),
            table: ResourceTable::new(),
            limits: StoreLimitsBuilder::new()
                .memory_size(256 * 1024 * 1024)
                .instances(32)
                .tables(32)
                .build(),
            permissions: manifest.permissions.clone(),
            processes: Processes::default(),
            workspace: PathBuf::from(&environment.workspace),
            data,
            assets,
            active: false,
            effects: vec![],
            staged_writes: Some(Default::default()),
        };
        let mut store = Store::new(engine, state);
        store.limiter(|s| &mut s.limits);
        store.set_fuel(100_000_000)?;
        let bindings = Plugin::instantiate(&mut store, &component, &linker)?;
        let mut instance = Self {
            next_call: 1,
            protocol: manifest.protocol,
            store,
            bindings,
            scene: None,
            scenes: Default::default(),
            panels: manifest.panels.iter().map(|p| p.id.clone()).collect(),
            error: None,
        };
        instance.call(Message::Prepare {
            environment,
            snapshot,
        })?;
        Ok(instance)
    }
    /// Every call gets a finite instruction budget; a trap cannot unwind through the host.
    pub fn call(&mut self, message: Message) -> anyhow::Result<Reply> {
        // Snapshot serialization may traverse the full configured scrollback, while input and
        // paint events stay on the smaller interactive budget. Both calls remain fuel-bounded.
        let fuel = if matches!(message, Message::Snapshot) {
            1_000_000_000
        } else {
            100_000_000
        };
        self.store.set_fuel(fuel)?;
        let Some((payload, invocation_id)) = self.encode_invocation(message)? else {
            // A capability guest receives only events belonging to this implemented native interface.
            return Ok(Reply::default());
        };
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
        for scene in reply.scene.iter().chain(&reply.scenes) {
            anyhow::ensure!(
                self.protocol >= 6
                    || !scene
                        .paint
                        .iter()
                        .any(|paint| matches!(paint, Paint::Svg { .. })),
                "Vector drawing requires manifest protocol 6 or newer"
            );
            // Protocol 3 scenes may deserialize their legacy `chrome` field into `controls`.
            anyhow::ensure!(
                scene.controls.is_none() || self.protocol >= 3,
                "Canvas controls require manifest protocol 3 or newer"
            );
            // An older host would silently dock left controls on the wrong side of the grid.
            anyhow::ensure!(
                self.protocol >= 5
                    || !scene.controls.as_ref().is_some_and(|controls| {
                        controls
                            .sidebar
                            .as_ref()
                            .is_some_and(|tabs| tabs.position == ui::SideTabsPosition::Left)
                    }),
                "Left sidebars require manifest protocol 5 or newer"
            );
            anyhow::ensure!(
                scene.ui.is_none() || self.protocol >= 2,
                "Native UI requires manifest protocol 2"
            );
            anyhow::ensure!(
                scene.paint.len() <= 200_000 && scene.widgets.len() <= 1024,
                "Scene quota exceeded"
            );
            validate_scene(scene)?;
            anyhow::ensure!(
                self.panels.contains(&scene.panel),
                "Undeclared panel: {}",
                scene.panel
            );
            anyhow::ensure!(
                panels.insert(&scene.panel),
                "Duplicate panel in plugin reply"
            );
        }
        // Publish only after every surface validates, so one invalid tree cannot partly update UI.
        for scene in reply.scene.iter().chain(&reply.scenes) {
            let scene = std::sync::Arc::new(scene.clone());
            self.scenes.insert(scene.panel.clone(), scene.clone());
            self.scene = Some(scene);
        }
        self.error = reply.error.clone();
        Ok(reply)
    }
    pub fn activate(&mut self) -> anyhow::Result<()> {
        self.store.data_mut().active = true;
        let reply = self.call(Message::Activate)?;
        anyhow::ensure!(
            reply.error.is_none(),
            "Plugin activation failed: {}",
            reply.error.unwrap_or_default()
        );
        Ok(())
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
    pub fn stop(&mut self) {
        self.store.data_mut().processes.clear();
        self.store.data_mut().active = false;
        self.store.data_mut().effects.clear();
    }
    pub fn process_count(&self) -> usize {
        self.store.data().processes.len()
    }
    pub fn process_ids(&self) -> Vec<u32> {
        self.store.data().processes.ids()
    }
    pub fn poll(&mut self) -> anyhow::Result<bool> {
        let events = self.store.data_mut().processes.poll()?;
        let changed = !events.is_empty();
        for event in events {
            self.call(Message::Event(event))?;
        }
        Ok(changed)
    }
    pub fn effects(&mut self) -> Vec<Request> {
        std::mem::take(&mut self.store.data_mut().effects)
    }
    pub fn snapshot(&mut self) -> anyhow::Result<Snapshot> {
        self.call(Message::Snapshot)?
            .snapshot
            .ok_or_else(|| anyhow::anyhow!("Plugin did not return its declared state"))
    }
}
