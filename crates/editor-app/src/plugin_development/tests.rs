//! Host templates exercise real form schemas, local overrides and isolation without plugin providers.
use super::{
    configuration::{self, Values},
    *,
};
use crate::*;
use gpui_kit::{TestAppContext, gpui};

fn resource(root: &Path) {
    std::fs::write(root.join("manifest.json"),r#"{"id":"test","name":"Test","version":"0.1.0","protocol":7,"api":{"base":"^1","required":{}},"contributions":"plugin.toml","permissions":[],"storage_limit":1024}"#).unwrap();
    std::fs::write(
        root.join("plugin.toml"),
        "[plugin]\nid='test'\nname='Test'\nversion='0.1.0'\nhost_version='>=0.1.0'\n",
    )
    .unwrap();
    std::fs::write(
        root.join("nanobug-plugin.json"),
        r#"{"version":1,"assets":[{"source":"plugin.toml","destination":"plugin.toml"}]}"#,
    )
    .unwrap();
}
/// The screenshot's missing description must identify the file/project in both UI languages,
/// and become valid once that same project receives its versioned build description.
#[test]
fn missing_project_description_is_actionable_and_recovers() {
    struct RestoreLocale(String);
    impl Drop for RestoreLocale {
        fn drop(&mut self) {
            rust_i18n::set_locale(&self.0);
        }
    }
    // Locale is process-wide; this regression runs with the host's sequential test convention.
    let _restore = RestoreLocale(rust_i18n::locale().to_string());
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().display().to_string();
    for locale in ["en", "zh-CN"] {
        rust_i18n::set_locale(locale);
        let (_, template) = configuration::templates(&workspace).remove(1);
        let arguments = serde_json::json!({"template":"development","values":template.defaults,"workspace":workspace,"intent":"run"});
        let validation: plugin_runtime::plugin_protocol::configurations::Validation =
            plugin_runtime::plugin_protocol::configurations::decode(
                configuration::validate(&arguments).unwrap(),
            )
            .unwrap();
        assert!(!validation.valid);
        assert!(
            validation.message.contains("nanobug-plugin.json"),
            "{}",
            validation.message
        );
        assert!(
            validation.message.contains(&workspace),
            "{}",
            validation.message
        );
        assert!(
            validation.message.contains(if locale == "en" {
                "Select"
            } else {
                "请选择"
            }),
            "{}",
            validation.message
        );
    }
    resource(root.path());
    let (_, template) = configuration::templates(&workspace).remove(1);
    let arguments = serde_json::json!({"template":"development","values":template.defaults,"workspace":workspace,"intent":"run"});
    let validation: plugin_runtime::plugin_protocol::configurations::Validation =
        plugin_runtime::plugin_protocol::configurations::decode(
            configuration::validate(&arguments).unwrap(),
        )
        .unwrap();
    assert!(validation.valid, "{}", validation.message);
}
/// A retained profile has one owner across controllers and resets, and becomes reusable on exit.
#[test]
fn profile_lease_rejects_a_second_owner_and_releases_on_drop() {
    let root = tempfile::tempdir().unwrap();
    let owner = instance::profile_lease(root.path()).unwrap();
    assert!(instance::profile_lease(root.path()).is_err());
    drop(owner);
    assert!(instance::profile_lease(root.path()).is_ok());
}
/// A changed ID cannot leave two development versions active; new authority needs another run.
#[test]
fn reload_requires_the_same_identity_and_granted_permissions() {
    let root = tempfile::tempdir().unwrap();
    resource(root.path());
    let mut manifest: plugin_runtime::plugin_protocol::Manifest =
        serde_json::from_slice(&std::fs::read(root.path().join("manifest.json")).unwrap()).unwrap();
    let descriptor = instance::Descriptor {
        session: "test".into(),
        development: manifest.id.clone(),
        profile: root.path().into(),
        workspace: root.path().into(),
        candidate: root.path().into(),
        grants: manifest.permissions.clone(),
    };
    assert!(descriptor.validate_candidate(&manifest).is_ok());
    manifest.id = "other".into();
    assert!(descriptor.validate_candidate(&manifest).is_err());
    manifest.id = descriptor.development.clone();
    manifest.permissions.insert("storage".into());
    assert!(descriptor.validate_candidate(&manifest).is_err());
}
/// The GUI and CLI share literal project/output values; selected paths never become shell fragments.
#[test]
fn host_templates_validate_and_keep_local_output_overrides() {
    let root = tempfile::tempdir().unwrap();
    resource(root.path());
    let workspace = root.path().display().to_string();
    for (_, template) in configuration::templates(&workspace) {
        let mut values: Values = serde_json::from_str(&template.defaults).unwrap();
        values.output = "output with spaces".into();
        let args = values
            .args(&template.id, &workspace, false, "test")
            .unwrap();
        if template.id == "package" {
            assert!(
                args.windows(2)
                    .any(|pair| pair[0] == "--output" && pair[1].ends_with("output with spaces"))
            );
        }
        let arguments = serde_json::json!({"template":template.id,"values":serde_json::to_string(&values).unwrap(),"workspace":workspace,"intent":"run"});
        let validation: plugin_runtime::plugin_protocol::configurations::Validation =
            plugin_runtime::plugin_protocol::configurations::decode(
                configuration::validate(&arguments).unwrap(),
            )
            .unwrap();
        assert!(validation.valid, "{}", validation.message);
        let form: plugin_runtime::plugin_protocol::configurations::Form =
            plugin_runtime::plugin_protocol::configurations::decode(
                configuration::form(&arguments).unwrap(),
            )
            .unwrap();
        form.document.validate().unwrap();
        let mut debug = arguments;
        debug["intent"] = "debug".into();
        let validation: plugin_runtime::plugin_protocol::configurations::Validation =
            plugin_runtime::plugin_protocol::configurations::decode(
                configuration::validate(&debug).unwrap(),
            )
            .unwrap();
        assert!(!validation.valid);
    }
    assert_ne!(
        profile(&workspace, "first").unwrap(),
        profile(&workspace, "copy").unwrap()
    );
}

/// Picker replacements become visible in an existing native input; ordinary typing keeps its epoch.
#[test]
fn picker_replacement_advances_only_the_changed_input() {
    let (_, template) = configuration::templates("project").remove(0);
    let mut arguments =
        serde_json::json!({"template":"package","values":template.defaults,"event":""});
    arguments["event"] = serde_json::to_string(
        &plugin_runtime::plugin_protocol::configurations::FormEvent::Native(
            plugin_runtime::plugin_protocol::ui::UiEvent {
                node: "picked-output".into(),
                action: plugin_runtime::plugin_protocol::ui::Action::Change(
                    "chosen directory".into(),
                ),
                revision: 0,
            },
        ),
    )
    .unwrap()
    .into();
    let reply: plugin_runtime::plugin_protocol::configurations::Form =
        plugin_runtime::plugin_protocol::configurations::decode(
            configuration::form(&arguments).unwrap(),
        )
        .unwrap();
    let values: Values = serde_json::from_str(&reply.values).unwrap();
    assert_eq!(values.output, "chosen directory");
    assert_eq!(values.picker_versions["output"], 1);
    assert!(!values.picker_versions.contains_key("name"));
    assert!(
        serde_json::to_string(&reply.document)
            .unwrap()
            .contains("\"value_revision\":1")
    );
}
/// Catalog, instantiation, edits and Apply work in the real native window with no installed plugins.
#[gpui::test]
fn host_templates_work_without_installed_plugins(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    resource(root.path());
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let workspace = Workspace::open(root.path()).unwrap();
    let slot = std::rc::Rc::new(std::cell::RefCell::new(None));
    let captured = slot.clone();
    let (_, visual) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *captured.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    visual.simulate_resize(size(px(1500.), px(900.)));
    visual.run_until_parked();
    let app = slot.borrow_mut().take().unwrap();
    crate::extensions::native_configuration_tests::open_form(&app, visual);
    crate::extensions::native_configuration_tests::click(visual, "run-config-add");
    crate::extensions::native_configuration_tests::click(
        visual,
        "run-template-$nanobug.plugin-development-package",
    );
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("plugin-ui-output").is_some());
    assert!(visual.debug_bounds("plugin-ui-output-browse").is_some());
    // A node can exist in the scene while its ancestor clips it to zero height.
    let viewport = visual
        .debug_bounds("plugin-ui-plugin-development-form")
        .unwrap();
    let input = visual.debug_bounds("plugin-ui-name").unwrap();
    assert!(
        viewport.size.height > px(100.),
        "Collapsed viewport: {viewport:?}"
    );
    assert!(
        viewport.contains(&input.center()),
        "Input outside visible viewport: {input:?}"
    );
    // Commit through the visible control, so tests exercise the same transaction as users.
    crate::extensions::native_configuration_tests::click(visual, "run-config-apply");
    visual.run_until_parked();
    visual.update(|_, cx| {
        let app = app.read(cx);
        let set = app.run_controls.configuration_set();
        let id = set
            .plugin_configurations
            .iter()
            .find(|(_, value)| value.provider == configuration::PROVIDER)
            .unwrap()
            .0;
        assert_eq!(
            set.plugin_configurations[id].validation,
            editor_core::ConfigurationValidation::Valid
        );
        assert!(app.run_controls.debug_blocker(&id).is_some());
        assert!(!set.find(&id).unwrap().build.is_empty());
    });
    let _ = std::fs::remove_file(
        editor_core::storage_path(&root.path().display().to_string()).unwrap(),
    );
}
