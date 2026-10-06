//! Fresh native installations exercise the shipped catalog, public Manager and real consent gestures.
use super::super::harness::NativeMarkdown;
use super::*;
use gpui_kit::VisualTestContext;
use std::io::{Cursor, Write};

/// Keep immutable package bytes available for a catalog without changing the public Package inspector.
pub(super) fn archive(package: &Package) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in &package.files {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// Test catalogs have an isolated location admitted only by cfg(test); production ignores project paths.
pub(super) fn catalog(directory: &Path, bytes: &[u8], extension: &str) {
    let root = directory.join(".bundled-plugin-test");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("delivered.zip"), bytes).unwrap();
    let package = Package::from_bytes(bytes).unwrap();
    std::fs::write(root.join("bundle-defaults.json"), serde_json::to_vec(&serde_json::json!({
        "version": 1, "packages": [{"file":"delivered.zip", "sha256":package.digest, "file_extensions":[extension]}]
    })).unwrap()).unwrap();
}

/// A fresh fixture deliberately installs nothing; its driver replays the production actor helpers.
pub(super) struct Fresh {
    pub(super) native: NativeMarkdown,
}

impl Fresh {
    /// Open a real private manager and native window with the same canonical workspace and trust.
    pub(super) fn mount<'a>(
        cx: &'a mut TestAppContext,
        trusted: bool,
        bytes: &[u8],
        extension: &str,
    ) -> (Self, &'a mut VisualTestContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            typography::init(cx);
            apply_theme(builtin_theme(false), cx);
            cx.set_reduce_motion(true);
        });
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join(format!("notes.{extension}")),
            "# 首次打开\n\n未保存编辑\n",
        )
        .unwrap();
        std::fs::write(directory.path().join("other.txt"), "plain text").unwrap();
        let workspace = Workspace::open(directory.path()).unwrap();
        catalog(workspace.root(), bytes, extension);
        let mut session = crate::app::session::SessionState::load(workspace.root());
        session.workspace_trusted = trusted;
        session.save();
        let manager = plugin_runtime::Manager::open_with_trust(
            workspace.root().join(".runtime-plugin-test"),
            protocol::Environment {
                workspace: workspace.root().display().to_string(),
                ..Default::default()
            },
            trusted,
        )
        .unwrap();
        let (app, ui) = NativeMarkdown::window(workspace, cx);
        (
            Self {
                native: NativeMarkdown {
                    directory,
                    manager,
                    renderer: images::VectorRenderer::default(),
                    app,
                },
            },
            ui,
        )
    }

    /// Only complete Manager.open and publication make the actor healthy, never an empty startup snapshot.
    pub(super) fn settle(&mut self, ui: &mut VisualTestContext) {
        for _ in 0..5 {
            let work: Vec<_> = ui.update(|_, cx| {
                self.native
                    .app
                    .read(cx)
                    .extensions
                    .read(cx)
                    .worker
                    .recorded
                    .lock()
                    .unwrap()
                    .try_iter()
                    .collect()
            });
            for work in work {
                match work {
                    Work::InspectBundle(request) => {
                        let token = request.token;
                        let result = bundled::prepare_offer(&mut self.native.manager, &request)
                            .map_err(|error| format!("{error:#}"));
                        ui.update(|_, cx| {
                            self.native
                                .app
                                .read(cx)
                                .extensions
                                .read(cx)
                                .worker
                                .state
                                .lock()
                                .unwrap()
                                .bundle_reply = Some(bundled::Reply { token, result })
                        });
                    }
                    Work::InstallBundle(candidate) => {
                        let trusted = ui.update(|_, cx| {
                            self.native
                                .app
                                .read(cx)
                                .extensions
                                .read(cx)
                                .worker
                                .trusted
                                .load(std::sync::atomic::Ordering::Acquire)
                        });
                        if trusted
                            && bundled::validate_candidate(&self.native.manager, &candidate).is_ok()
                        {
                            self.native
                                .manager
                                .install(
                                    candidate.package.as_ref(),
                                    candidate.package.manifest.permissions.clone(),
                                )
                                .unwrap();
                            language_tests::publish_languages(
                                &self.native.app,
                                &self.native.manager,
                                ui,
                            );
                        }
                        ui.update(|_, cx| {
                            let owner = self.native.app.read(cx).extensions.read(cx);
                            let mut state = owner.worker.state.lock().unwrap();
                            state.progress = None;
                            state.installation = None;
                            state.install_control = None;
                        });
                    }
                    Work::DeclineBundle(candidate) => {
                        self.native
                            .manager
                            .record_bundle_decline(&candidate.package.manifest.id)
                            .unwrap();
                    }
                    Work::SetTrust(trusted) => {
                        self.native.manager.set_workspace_trust(trusted).unwrap()
                    }
                    Work::Event(id, _, panel, event) => {
                        if let Err(error) = self.native.manager.event(&id, panel, event) {
                            assert!(
                                matches!(error.downcast_ref::<protocol::api::Failure>(), Some(error)
                                if error.code == protocol::api::ErrorCode::StaleRevision),
                                "{error:#}"
                            );
                        }
                    }
                    _ => {}
                }
            }
            self.native.manager.poll();
            ui.update(|_, cx| {
                self.native
                    .app
                    .read(cx)
                    .extensions
                    .read(cx)
                    .worker
                    .state
                    .lock()
                    .unwrap()
                    .ready = true
            });
            super::super::composable_tests::publish(
                &mut self.native.manager,
                &mut self.native.renderer,
                &self.native.app,
                ui,
            );
        }
    }

    /// Open through the explorer/tab path while leaving the production inspection request queued.
    pub(super) fn open(&mut self, name: &str, ui: &mut VisualTestContext) {
        let path = self.native.directory.path().join(name);
        ui.update(|window, cx| {
            self.native
                .app
                .update(cx, |app, cx| app.open_file(path, window, cx))
        });
        ui.run_until_parked();
        self.settle(ui);
    }

    /// Inspect the real Base overlay inside the editor Root, without setting a pending package in the test.
    pub(super) fn consent(&self, ui: &mut VisualTestContext) -> bool {
        ui.debug_bounds("plugin-install-consent").is_some()
    }

    /// Dispatch a real confirmation/cancellation; the recorded production work is handled on the next settle.
    pub(super) fn click_consent(&self, selector: &'static str, ui: &mut VisualTestContext) {
        assert!(self.consent(ui), "native first-use permission dialog");
        let bounds = ui.debug_bounds(selector).expect("real permission control");
        ui.simulate_click(bounds.center(), Default::default());
        ui.run_until_parked();
    }

    /// Restart both native host and actual public runtime over the same retained private store.
    pub(super) fn reopen<'a>(
        self,
        cx: &'a mut TestAppContext,
    ) -> (Self, &'a mut VisualTestContext) {
        let NativeMarkdown {
            directory, manager, ..
        } = self.native;
        drop(manager);
        let workspace = Workspace::open(directory.path()).unwrap();
        let trusted = crate::app::session::SessionState::load(workspace.root()).workspace_trusted;
        let manager = plugin_runtime::Manager::open_with_trust(
            workspace.root().join(".runtime-plugin-test"),
            protocol::Environment {
                workspace: workspace.root().display().to_string(),
                ..Default::default()
            },
            trusted,
        )
        .unwrap();
        let (app, ui) = NativeMarkdown::window(workspace, cx);
        let mut fixture = Self {
            native: NativeMarkdown {
                directory,
                manager,
                renderer: images::VectorRenderer::default(),
                app,
            },
        };
        fixture.settle(ui);
        (fixture, ui)
    }
}

/// Reuse the actual external SDK component with a native preview declaration for any unfamiliar extension.
pub(super) fn independent_preview(extension: &str) -> Package {
    let mut files = Package::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/plugin-sdk-test/capability-example.zip"),
    )
    .unwrap()
    .files;
    let mut manifest: serde_json::Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["id"] = serde_json::json!("bundled-opaque-preview");
    manifest["settings_hook"] = serde_json::json!(false);
    manifest["api"]["required"]["editor.layout"] = serde_json::json!("^1");
    manifest["settings"]["label"]["default"] = serde_json::json!("composable-ui");
    manifest["panels"][0]["position"] = serde_json::json!("editor");
    manifest["panels"][0]["file_extensions"] = serde_json::json!([extension]);
    manifest["panels"][0]["default_visible"] = serde_json::json!(true);
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    // The unfamiliar guest owns its split and explicitly borrows the existing source once.
    let editor = protocol::ui::Node::new(
        "bundle-editor",
        protocol::ui::Kind::NativeEditor {
            document: protocol::api::DocumentVersion {
                id: "template".into(),
                path: "template".into(),
                revision: 0,
            },
        },
    )
    .grow();
    let mut document = protocol::ui::Document::new(
        protocol::ui::Node::row(
            "bundle-layout",
            vec![
                editor,
                protocol::ui::Node::text("bundle-greeting", "Independent public SDK preview")
                    .grow(),
            ],
        )
        .grow(),
    );
    document.editor_layout = true;
    files.insert(
        "composed-ui.json".into(),
        serde_json::to_vec(&document).unwrap(),
    );
    language_tests::packages::repack(files).unwrap()
}

/// Read the physically delivered SDK-built ZIP; all UI tests are opt-in until that artifact exists.
pub(super) fn delivered() -> Vec<u8> {
    std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/markdown.zip"))
        .unwrap()
}
