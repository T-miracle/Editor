//! Public packages, actual guest output and durable settings verify shortcut lifecycle ownership.

use super::*;
use gpui_kit::{KeyDownEvent, Keystroke, PlatformInput};

const PLUGIN: &str = "shortcut-query-fixture";
const SHOW_ROW: &str = "shortcut-operation-shortcut-query-fixture/show-panel";
const SHOW_BINDING: &str = "shortcut-binding-shortcut-query-fixture/show-panel-0";
const SHOW_CONFLICT: &str = "shortcut-conflict-shortcut-query-fixture/show-panel";
const SHOW_RESOLVE: &str = "shortcut-resolve-shortcut-query-fixture/show-panel";
const CHECK_BINDING: &str = "shortcut-binding-shortcut-query-fixture/check-errors-0";
const CHECK_RESULT: &str = "Typed errors and request IDs verified.";

/// Only fixture declarations vary; every variant retains the independently built SDK component.
fn lifecycle_package(remove_show: bool, version: &str) -> Package {
    let mut files = shortcut_package().files;
    let mut manifest: serde_json::Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["version"] = serde_json::json!(version);
    let commands = manifest["commands"].as_array_mut().unwrap();
    if remove_show {
        commands.retain(|command| command["id"] != "show-panel");
    }
    commands.push(serde_json::json!({
        "id":"check-errors", "title":"Shortcut fixture check errors", "shortcut":"ctrl-alt-8"
    }));
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        archive
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(&bytes).unwrap();
    }
    Package::from_bytes(&archive.finish().unwrap().into_inner()).unwrap()
}

/// The fixture exposes real guest effects through published UI and the production user file.
struct Fixture {
    app: Entity<EditorApp>,
    manager: plugin_runtime::Manager,
    renderer: images::VectorRenderer,
    profile: PathBuf,
    document: PathBuf,
}

impl Fixture {
    /// Reconcile a manager frame without generating another user keystroke.
    fn publish(&mut self, visual: &mut VisualTestContext) {
        composable_tests::publish(&mut self.manager, &mut self.renderer, &self.app, visual);
        draw(visual);
    }

    /// Process the application's ordinary transport and publish the actual guest's result.
    fn settle(&mut self, visual: &mut VisualTestContext) {
        composable_tests::pump(&mut self.manager, &self.app, visual);
        self.publish(visual);
    }

    /// Close the public panel and restore editor focus before an application shortcut.
    fn focus_editor(&self, visual: &mut VisualTestContext) {
        hide_and_focus_editor(visual, &self.app, "welcome");
    }

    /// Read text from the same declarative document that the native panel displays.
    fn guest_text(&self) -> String {
        serde_json::to_string(&self.manager.live[PLUGIN].views["welcome"]).unwrap()
    }

    /// Inspect durable storage, rather than the engine's in-memory override map.
    fn saved_binding(&self, command: &str) -> serde_json::Value {
        let profile: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&self.profile).unwrap()).unwrap();
        let id = format!("plugin:{}", serde_json::json!([PLUGIN, command]));
        profile["overrides"][id].clone()
    }
}

/// Private storage and the explicit user profile remain alive throughout each application scenario.
fn with_plugin(
    cx: &mut TestAppContext,
    dark: bool,
    scenario: impl FnOnce(&mut VisualTestContext, &mut Fixture),
) {
    let profile_root = tempfile::tempdir().unwrap();
    let private_root = tempfile::tempdir().unwrap();
    let profile = profile_root.path().join("shortcuts.json");
    crate::tests::with_shortcut_editor_profile(
        cx,
        dark,
        vec![gpui_kit::KeyBinding::new(
            "ctrl-alt-u",
            crate::SaveDocument,
            Some("EditorShell && !PluginSurface"),
        )],
        &profile,
        |visual, app, path| {
            let package = lifecycle_package(false, "0.16.1");
            let mut manager = plugin_runtime::Manager::open(
                private_root.path().to_path_buf(),
                protocol::Environment {
                    workspace: path.parent().unwrap().display().to_string(),
                    ..Default::default()
                },
            )
            .unwrap();
            manager
                .install(&package, package.manifest.permissions.clone())
                .unwrap();
            let mut fixture = Fixture {
                app,
                manager,
                renderer: images::VectorRenderer::default(),
                profile: profile.clone(),
                document: path.to_path_buf(),
            };
            fixture.publish(visual);
            fixture.focus_editor(visual);
            scenario(visual, &mut fixture);
        },
    );
}

/// Capture the document/plugin focus before opening the ordinary global query.
fn open_query(visual: &mut VisualTestContext, title: &str) {
    visual.simulate_keystrokes("ctrl-k alt-right");
    draw(visual);
    visual.simulate_input(title);
    draw(visual);
}

/// Use the visible inline capture and explicit Save controls for each binding replacement.
fn rebind(visual: &mut VisualTestContext, title: &str, selector: &'static str, keys: &str) {
    open_query(visual, title);
    click(visual, selector);
    visual.simulate_keystrokes(keys);
    visual.executor().advance_clock(Duration::from_secs(2));
    draw(visual);
    click(visual, "shortcuts-edit-save");
    assert!(visual.debug_bounds("shortcuts-edit-capture").is_none());
    visual.simulate_keystrokes("escape");
    draw(visual);
}

/// A real document save distinguishes the current owner from a paused plugin shortcut.
fn save_document(visual: &mut VisualTestContext, fixture: &Fixture, keys: &str, text: &str) {
    visual.simulate_keystrokes("ctrl-a");
    visual.simulate_input(text);
    visual.simulate_keystrokes(keys);
    draw(visual);
    assert_eq!(std::fs::read_to_string(&fixture.document).unwrap(), text);
}

/// Retirement refreshes an open catalog/draft, retains settings, and restores only live commands.
#[gpui::test]
#[ignore = "build the real SDK fixture with scripts/build-capability-example.ps1 first"]
fn shortcuts_real_plugin_lifecycle_disable_removes_open_rows(cx: &mut TestAppContext) {
    with_plugin(cx, false, |visual, fixture| {
        rebind(
            visual,
            "Shortcut fixture show panel",
            SHOW_BINDING,
            "ctrl-alt-v",
        );
        let saved = std::fs::read(&fixture.profile).unwrap();
        assert_eq!(
            fixture.saved_binding("show-panel"),
            serde_json::json!([["ctrl-alt-v"]])
        );
        open_query(visual, "Shortcut fixture");
        assert!(visual.debug_bounds(SHOW_ROW).is_some());
        click(visual, SHOW_BINDING);
        visual.simulate_keystrokes("ctrl-alt-b");
        draw(visual);

        fixture.manager.disable(PLUGIN).unwrap();
        fixture.publish(visual);
        assert!(visual.debug_bounds("shortcuts-panel").is_some());
        assert!(
            visual.debug_bounds(SHOW_ROW).is_none(),
            "retirement must refresh before another key"
        );
        assert!(visual.debug_bounds("shortcuts-edit-capture").is_none());
        assert!(visual.debug_bounds("shortcuts-edit-error").is_some());
        assert_eq!(std::fs::read(&fixture.profile).unwrap(), saved);
        visual.simulate_keystrokes("escape ctrl-alt-v");
        fixture.settle(visual);
        assert!(visual.debug_bounds("plugin-ui-welcome-text").is_none());

        fixture.manager.enable(PLUGIN).unwrap();
        fixture.publish(visual);
        fixture.focus_editor(visual);
        visual.simulate_keystrokes("ctrl-alt-v");
        draw(visual);
        fixture.settle(visual);
        assert!(visual.debug_bounds("plugin-ui-welcome-text").is_some());
        assert_eq!(
            fixture.saved_binding("show-panel"),
            serde_json::json!([["ctrl-alt-v"]])
        );

        fixture.manager.uninstall(PLUGIN, false).unwrap();
        fixture.publish(visual);
        assert_eq!(std::fs::read(&fixture.profile).unwrap(), saved);
        // The retained key stays inert while a newer declaration omits its original command.
        let removed = lifecycle_package(true, "0.16.2");
        fixture
            .manager
            .install(&removed, removed.manifest.permissions.clone())
            .unwrap();
        fixture.publish(visual);
        fixture.focus_editor(visual);
        open_query(visual, "Shortcut fixture");
        assert!(visual.debug_bounds(SHOW_ROW).is_none());
        assert!(
            visual
                .debug_bounds("shortcut-operation-shortcut-query-fixture/read-selection")
                .is_some()
        );
        visual.simulate_keystrokes("escape ctrl-alt-v");
        fixture.settle(visual);
        assert!(visual.debug_bounds("plugin-ui-welcome-text").is_none());
        assert_eq!(std::fs::read(&fixture.profile).unwrap(), saved);

        let restored = lifecycle_package(false, "0.16.3");
        fixture
            .manager
            .install(&restored, restored.manifest.permissions.clone())
            .unwrap();
        fixture.publish(visual);
        fixture.focus_editor(visual);
        visual.simulate_keystrokes("ctrl-alt-v");
        draw(visual);
        fixture.settle(visual);
        assert!(visual.debug_bounds("plugin-ui-welcome-text").is_some());
        assert_eq!(std::fs::read(&fixture.profile).unwrap(), saved);
    });
}

/// Exact and prefix conflicts pause returning plugins without stealing an existing save key.
#[gpui::test]
#[ignore = "build the real SDK fixture with scripts/build-capability-example.ps1 first"]
fn shortcuts_real_plugin_lifecycle_restoration_conflicts_require_decision(cx: &mut TestAppContext) {
    with_plugin(cx, true, |visual, fixture| {
        for (plugin_keys, native_key) in [("ctrl-alt-v", "ctrl-alt-v"), ("ctrl-j ctrl-v", "ctrl-j")]
        {
            fixture.focus_editor(visual);
            rebind(
                visual,
                "Shortcut fixture show panel",
                SHOW_BINDING,
                plugin_keys,
            );
            fixture.manager.disable(PLUGIN).unwrap();
            fixture.publish(visual);
            let title = rust_i18n::t!("shortcuts.operation.SaveDocument").to_string();
            open_query(visual, &title);
            click(visual, "shortcut-add-me_editor::SaveDocument");
            visual.simulate_keystrokes(native_key);
            visual.executor().advance_clock(Duration::from_secs(2));
            draw(visual);
            click(visual, "shortcuts-edit-save");
            visual.simulate_keystrokes("escape");
            draw(visual);

            fixture.manager.enable(PLUGIN).unwrap();
            fixture.publish(visual);
            fixture.focus_editor(visual);
            save_document(visual, fixture, native_key, "existing owner stays active");
            fixture.settle(visual);
            assert!(visual.debug_bounds("plugin-ui-welcome-text").is_none());
            open_query(visual, "Shortcut fixture show panel");
            assert!(visual.debug_bounds(SHOW_CONFLICT).is_some());
            assert!(
                visual.debug_bounds(SHOW_BINDING).is_some(),
                "paused configuration remains editable"
            );
            let before_decision = std::fs::read(&fixture.profile).unwrap();
            click(visual, SHOW_RESOLVE);
            assert!(visual.debug_bounds("shortcuts-edit-confirmation").is_some());
            click(visual, "shortcuts-edit-continue");
            assert_eq!(std::fs::read(&fixture.profile).unwrap(), before_decision);
            click(visual, SHOW_RESOLVE);
            click(visual, "shortcuts-edit-replace");
            draw(visual);
            assert!(visual.debug_bounds(SHOW_CONFLICT).is_none());
            visual.simulate_keystrokes("escape");
            draw(visual);
            save_document(
                visual,
                fixture,
                "ctrl-alt-u",
                "unrelated save binding survives",
            );
            visual.simulate_keystrokes("ctrl-a");
            visual.simulate_input("plugin shortcut cannot save this");
            visual.simulate_keystrokes(plugin_keys);
            draw(visual);
            fixture.settle(visual);
            assert!(visual.debug_bounds("plugin-ui-welcome-text").is_some());
            assert_eq!(
                std::fs::read_to_string(&fixture.document).unwrap(),
                "unrelated save binding survives"
            );
        }
    });
}

/// Same-digest replacement retires waiting/deferred/queued commands, and restriction cannot restart it.
#[gpui::test]
#[ignore = "build the real SDK fixture with scripts/build-capability-example.ps1 first"]
fn shortcuts_real_plugin_lifecycle_epochs_and_restricted_workspace(cx: &mut TestAppContext) {
    with_plugin(cx, false, |visual, fixture| {
        // Actual guest text proves the command ran, independently of the host revealing its panel.
        visual.simulate_keystrokes("ctrl-alt-8");
        draw(visual);
        fixture.settle(visual);
        assert!(visual.debug_bounds("plugin-ui-welcome-text").is_some());
        assert!(fixture.guest_text().contains(CHECK_RESULT));
        visual.simulate_keystrokes("ctrl-k");
        draw(visual);
        visual.simulate_input("Shortcut fixture check errors");
        draw(visual);
        assert!(
            visual
                .debug_bounds("shortcut-operation-shortcut-query-fixture/check-errors")
                .is_some()
        );
        visual.simulate_keystrokes("escape");
        draw(visual);
        assert!(visual.update(|window, cx| {
            fixture.app.read(cx).plugin_panels["shortcut-query-fixture/welcome"]
                .read(cx)
                .focus
                .is_focused(window)
        }));

        fixture.focus_editor(visual);
        rebind(
            visual,
            "Shortcut fixture check errors",
            CHECK_BINDING,
            "ctrl-alt-8 ctrl-alt-9",
        );
        visual.simulate_keystrokes("ctrl-alt-8");
        draw(visual);
        assert!(visual.debug_bounds("shortcuts-pending").is_some());
        let digest = fixture.manager.installed[PLUGIN].digest.clone();
        let previous_instance = fixture.manager.instance_id(PLUGIN).unwrap().to_owned();
        fixture.manager.restart_plugin(PLUGIN).unwrap();
        assert_eq!(fixture.manager.installed[PLUGIN].digest, digest);
        assert_ne!(
            fixture.manager.instance_id(PLUGIN).unwrap(),
            previous_instance
        );
        fixture.publish(visual);
        assert!(
            visual.debug_bounds("shortcuts-pending").is_none(),
            "replacement immediately clears its prefix"
        );
        visual.simulate_keystrokes("ctrl-alt-9");
        draw(visual);
        fixture.settle(visual);
        assert!(!fixture.guest_text().contains(CHECK_RESULT));

        // Host callers retain their original epoch until the worker admits the queued command.
        visual.update(|window, cx| {
            fixture.app.update(cx, |app, cx| {
                app.invoke_plugin_command(
                    PLUGIN,
                    "check-errors",
                    serde_json::Value::Null,
                    window,
                    cx,
                )
                .unwrap();
            })
        });
        fixture.manager.restart_plugin(PLUGIN).unwrap();
        fixture.settle(visual);
        assert!(
            !fixture.guest_text().contains(CHECK_RESULT),
            "queued command cannot enter a replacement"
        );

        // Replace and publish between actual key resolution and the deferred UI callback.
        fixture.focus_editor(visual);
        rebind(
            visual,
            "Shortcut fixture check errors",
            CHECK_BINDING,
            "ctrl-alt-8",
        );
        visual.update(|window, cx| {
            window.dispatch_event(
                PlatformInput::KeyDown(KeyDownEvent {
                    keystroke: Keystroke::parse("ctrl-alt-8").unwrap(),
                    is_held: false,
                    prefer_character_input: false,
                }),
                cx,
            );
            fixture.manager.restart_plugin(PLUGIN).unwrap();
            let extensions = fixture.app.read(cx).extensions.clone();
            extensions.update(cx, |owner, cx| {
                owner
                    .worker
                    .state
                    .lock()
                    .unwrap()
                    .publish_manager(&fixture.manager);
                owner.poll(cx);
            });
        });
        fixture.settle(visual);
        assert!(
            !fixture.guest_text().contains(CHECK_RESULT),
            "deferred command retains its original epoch"
        );
        fixture.focus_editor(visual);
        visual.simulate_keystrokes("ctrl-alt-8");
        draw(visual);
        fixture.settle(visual);
        assert!(fixture.guest_text().contains(CHECK_RESULT));

        // Trust changes use existing application and Manager APIs, outside shortcut management.
        fixture.focus_editor(visual);
        visual.update(|_, cx| {
            fixture
                .app
                .update(cx, |app, cx| app.set_workspace_trusted(false, cx))
        });
        fixture.manager.set_workspace_trust(false).unwrap();
        fixture.publish(visual);
        assert!(fixture.manager.live.is_empty());
        let retained = fixture.saved_binding("check-errors");
        open_query(visual, "Shortcut fixture");
        assert!(visual.debug_bounds("shortcuts-empty").is_some());
        visual.simulate_keystrokes("escape");
        draw(visual);
        rebind(
            visual,
            &rust_i18n::t!("shortcuts.operation.SaveDocument"),
            "shortcut-binding-me_editor::SaveDocument-0",
            "ctrl-alt-s",
        );
        visual.simulate_keystrokes("ctrl-alt-8");
        draw(visual);
        fixture.settle(visual);
        assert!(
            fixture.manager.live.is_empty(),
            "editing keys cannot restart a restricted plugin"
        );
        assert_eq!(fixture.saved_binding("check-errors"), retained);
        assert!(!visual.update(|_, cx| fixture.app.read(cx).session_state.workspace_trusted));
        save_document(
            visual,
            fixture,
            "ctrl-alt-s",
            "restricted workspace still edits",
        );
    });
}
