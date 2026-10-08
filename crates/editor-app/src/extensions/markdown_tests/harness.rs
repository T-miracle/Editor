//! Delivered Markdown packages share a real manager, isolated data and native publication driver.
use super::*;
use gpui_kit::VisualTestContext;

/// Inspect the delivered ZIP so all native scenarios execute its real component and resources.
fn delivered_package() -> Package {
    Package::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/markdown.zip"),
    )
    .unwrap()
}

/// Remove one declared permission before public installation, preserving the actual guest and assets.
/// A legal smaller declaration lets its SDK operation fail without bypassing startup grant checks.
pub(super) fn package_without_permission(permission: &str) -> Package {
    let mut files = delivered_package().files;
    let mut manifest: serde_json::Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    let permissions = manifest["permissions"].as_array_mut().unwrap();
    let original_len = permissions.len();
    permissions.retain(|declared| declared.as_str() != Some(permission));
    assert_eq!(
        permissions.len() + 1,
        original_len,
        "permission was declared once"
    );
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    language_tests::packages::repack(files).unwrap()
}

pub(super) struct NativeMarkdown {
    pub directory: tempfile::TempDir,
    pub manager: plugin_runtime::Manager,
    pub renderer: images::VectorRenderer,
    pub app: Entity<EditorApp>,
}

impl NativeMarkdown {
    /// Observe the guest's published selected state rather than an obsolete host preference field.
    pub fn selected_tool(&self, id: &str) -> bool {
        self.manager.live["markdown"].views["preview"]
            .tools
            .iter()
            .find(|tool| tool.id == id)
            .unwrap()
            .selected
    }
    /// Mount the public ZIP with the same private root used by the existing native test worker.
    pub fn mount<'a>(
        cx: &'a mut TestAppContext,
        files: &[(&str, &str)],
    ) -> (Self, &'a mut VisualTestContext) {
        Self::mount_package(cx, files, &delivered_package())
    }

    /// Mount an inspected real package with exactly its declared grants in an isolated native workspace.
    /// Package variants enter through Manager.install; no live Store permissions or host API are patched.
    pub fn mount_package<'a>(
        cx: &'a mut TestAppContext,
        files: &[(&str, &str)],
        package: &Package,
    ) -> (Self, &'a mut VisualTestContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            typography::init(cx);
            apply_theme(builtin_theme(false), cx);
            cx.set_reduce_motion(true);
        });
        let directory = tempfile::tempdir().unwrap();
        for (name, text) in files {
            std::fs::write(directory.path().join(name), text).unwrap();
        }
        let workspace = Workspace::open(directory.path()).unwrap();
        let mut manager = plugin_runtime::Manager::open(
            workspace.root().join(".runtime-plugin-test"),
            protocol::Environment {
                // Match production metadata so an ordinary process plugin chooses the native shell.
                os: std::env::consts::OS.into(),
                workspace: workspace.root().display().to_string(),
                ..Default::default()
            },
        )
        .unwrap();
        manager
            .install(package, package.manifest.permissions.clone())
            .unwrap();
        let (app, ui) = Self::window(workspace, cx);
        let mut fixture = Self {
            directory,
            manager,
            renderer: images::VectorRenderer::default(),
            app,
        };
        fixture.settle(ui);
        (fixture, ui)
    }

    /// A fresh native window restores preferences through the production workspace session loader.
    pub fn window(
        workspace: Workspace,
        cx: &mut TestAppContext,
    ) -> (Entity<EditorApp>, &mut VisualTestContext) {
        let slot = Rc::new(RefCell::new(None));
        let capture = slot.clone();
        let (_, ui) = cx.add_window_view(move |window, cx| {
            let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
            *capture.borrow_mut() = Some(app.clone());
            Root::new(app, window, cx)
        });
        // Native focus listeners describe the foreground window; synthetic key input alone does not activate it.
        ui.update(|window, _| window.activate_window());
        ui.run_until_parked();
        ui.simulate_resize(size(px(1400.), px(900.)));
        (slot.borrow_mut().take().unwrap(), ui)
    }

    /// Let source notifications and rendered trees cross the existing worker seam in both directions.
    pub fn settle(&mut self, cx: &mut VisualTestContext) {
        use super::super::composable_tests::{publish, pump};
        for _ in 0..5 {
            // Advance the real publication cadence deterministically instead of sleeping in native tests.
            cx.executor().advance_clock(Duration::from_millis(160));
            cx.run_until_parked();
            pump(&mut self.manager, &self.app, cx);
            self.manager.poll();
            let requests = self
                .manager
                .live
                .iter_mut()
                .flat_map(|(id, instance)| {
                    instance
                        .take_editor_requests()
                        .into_iter()
                        .map(|request| (id.clone(), request))
                })
                .collect::<Vec<_>>();
            // The production worker hands these owned handles to its deferred editor queue.
            // Tests drive that same queue; they never apply text or synthesize request results.
            cx.update(|_, cx| {
                self.app
                    .read(cx)
                    .extensions
                    .read(cx)
                    .worker
                    .state
                    .lock()
                    .unwrap()
                    .editor_requests
                    .extend(requests);
            });
            publish(&mut self.manager, &mut self.renderer, &self.app, cx);
        }
    }

    /// Open a fixture document through the same path as an explorer or tab activation.
    pub fn open(&mut self, name: &str, cx: &mut VisualTestContext) {
        let path = self.directory.path().join(name);
        cx.update(|window, cx| {
            self.app
                .update(cx, |app, cx| app.open_file(path, window, cx))
        });
        cx.run_until_parked();
        self.settle(cx);
    }

    /// Activate a visible control with native pointer dispatch, then deliver its plugin effects.
    pub fn click(&mut self, selector: &'static str, cx: &mut VisualTestContext) {
        let position = cx.debug_bounds(selector).expect("visible control").center();
        cx.simulate_click(position, Default::default());
        cx.run_until_parked();
        self.settle(cx);
    }

    /// Focus the real source editor before testing its normal clipboard and keyboard behavior.
    pub fn focus_editor(&self, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            self.app
                .read(cx)
                .editor
                .clone()
                .update(cx, |editor, cx| editor.focus(window, cx))
        });
    }
}
