//! Delivered UI packages cross the same installed-package and addressed-event boundary as third parties.
use plugin_runtime::{
    Manager, Package,
    plugin_protocol::{Environment, Paint, api, ui},
};
use std::path::Path;

fn package(id: &str) -> Package {
    Package::read(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../dist/plugins/{id}.zip")),
    )
    .unwrap()
}
fn manager(root: &Path) -> Manager {
    Manager::open(
        root.join("plugins"),
        Environment {
            workspace: root.display().to_string(),
            ..Default::default()
        },
    )
    .unwrap()
}
/// Events retain their declared panel; tests cannot reach guest internals or bypass permissions.
fn notify(manager: &mut Manager, id: &str, panel: &str, event: api::Notification) {
    manager.event(id, Some(panel.into()), event).unwrap();
}
fn tree<'a>(manager: &'a Manager, id: &str, panel: &str) -> &'a ui::Document {
    manager.live[id].views[panel].as_ref()
}
fn action(manager: &mut Manager, id: &str, panel: &str, node: &str, action: ui::Action) {
    let revision = tree(manager, id, panel).revision;
    notify(
        manager,
        id,
        panel,
        api::Notification::Ui(ui::UiEvent {
            revision,
            node: node.into(),
            action,
        }),
    );
}

/// A text-capable image receives file identity before its exact memory snapshot, like the native host.
fn svg_preview(manager: &mut Manager, document: Option<api::DocumentVersion>, text: &str) {
    let file = document.as_ref().map(|source| api::FileContext {
        version: api::FileVersion {
            id: format!("file:{}", source.id),
            path: source.path.clone(),
            revision: source.revision,
        },
        file_type: "svg".into(),
        text: Some(source.clone()),
    });
    notify(
        manager,
        "svg",
        "preview",
        api::Notification::FilePreview { file },
    );
    notify(
        manager,
        "svg",
        "preview",
        api::Notification::Preview {
            document,
            text: text.into(),
        },
    );
}

/// No native permission is needed for snapshots, counters, notes or standard controls.
#[test]
#[ignore = "build example and svg packages through the host SDK first"]
fn example_installs_without_grants_and_restores_notes_and_count() {
    let package = package("example");
    assert_eq!(package.manifest.protocol, 7);
    assert!(package.manifest.permissions.is_empty());
    let root = tempfile::tempdir().unwrap();
    let mut manager = manager(root.path());
    manager.install(&package, Default::default()).unwrap();
    action(
        &mut manager,
        "example",
        "counter",
        "increment",
        ui::Action::Click,
    );
    // A native input can commit several changes from one frame; value updates keep its target revision.
    let revision = tree(&manager, "example", "notes").revision;
    for value in ["中文", "中文 notes with spaces"] {
        notify(
            &mut manager,
            "example",
            "notes",
            api::Notification::Ui(ui::UiEvent {
                revision,
                node: "note".into(),
                action: ui::Action::Change(value.into()),
            }),
        );
    }
    manager.disable("example").unwrap();
    manager.enable("example").unwrap();
    assert!(
        serde_json::to_string(tree(&manager, "example", "counter"))
            .unwrap()
            .contains("点击次数：1")
    );
    assert!(
        serde_json::to_string(tree(&manager, "example", "notes"))
            .unwrap()
            .contains("中文 notes with spaces")
    );
    action(
        &mut manager,
        "example",
        "counter",
        "open-dialog",
        ui::Action::Click,
    );
    assert!(tree(&manager, "example", "counter").dialog.is_some());
    action(
        &mut manager,
        "example",
        "counter",
        "sample-dialog",
        ui::Action::Dismiss,
    );
    assert!(tree(&manager, "example", "counter").dialog.is_none());
    manager.uninstall("example", true).unwrap();
    assert!(manager.live.is_empty());
}

const SVG: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"200\" height=\"100\"><circle r=\"30\"/></svg>";
/// Memory text is explicitly authorized and echoed with the exact source identity, never read from disk.
#[test]
#[ignore = "build example and svg packages through the host SDK first"]
fn svg_uses_versioned_memory_and_discards_obsolete_content() {
    let package = package("svg");
    assert!(
        package.manifest.permissions.contains("editor.read")
            && package.manifest.permissions.contains("storage")
    );
    let root = tempfile::tempdir().unwrap();
    let mut manager = manager(root.path());
    assert!(manager.install(&package, Default::default()).is_err());
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let version = api::DocumentVersion {
        id: "open-svg".into(),
        path: "unsaved.svg".into(),
        revision: 7,
    };
    svg_preview(&mut manager, Some(version.clone()), SVG);
    let view = tree(&manager, "svg", "preview");
    assert_eq!(view.source.as_ref(), Some(&version));
    let vector = |manager: &Manager| {
        let ui::Kind::Canvas(canvas) = &tree(manager, "svg", "preview")
            .active_node("preview-canvas")
            .unwrap()
            .kind
        else {
            panic!("canvas required")
        };
        canvas
            .paint
            .iter()
            .rev()
            .find_map(|paint| match paint {
                Paint::Svg { rect, source, .. } if source == SVG => Some(*rect),
                _ => None,
            })
            .unwrap()
    };
    let initial = vector(&manager);
    action(
        &mut manager,
        "svg",
        "preview",
        "preview-canvas",
        ui::Action::Canvas(ui::CanvasEvent::Wheel {
            delta_x: 0.,
            delta_y: 14.,
            shift: false,
            x: 200.,
            y: 150.,
        }),
    );
    assert!(vector(&manager).w > initial.w);
    let error = manager
        .event(
            "svg",
            Some("preview".into()),
            api::Notification::Preview {
                document: Some(api::DocumentVersion {
                    revision: 6,
                    ..version.clone()
                }),
                text: "obsolete".into(),
            },
        )
        .unwrap_err();
    assert_eq!(
        error
            .downcast_ref::<api::Failure>()
            .map(|failure| failure.code),
        Some(api::ErrorCode::StaleRevision)
    );
    assert!(manager.installed["svg"].error.is_none());
    assert_eq!(
        tree(&manager, "svg", "preview").source.as_ref(),
        Some(&version)
    );
    assert!(vector(&manager).w > initial.w);
    // A new editor entity at the same path starts at revision zero and resets view state.
    let reopened = api::DocumentVersion {
        id: "reopened".into(),
        revision: 0,
        ..version
    };
    svg_preview(&mut manager, Some(reopened.clone()), SVG);
    assert_eq!(
        tree(&manager, "svg", "preview").source.as_ref(),
        Some(&reopened)
    );
    assert_eq!(vector(&manager).w, initial.w);
    svg_preview(&mut manager, None, "");
    assert!(tree(&manager, "svg", "preview").source.is_none());
    manager.disable("svg").unwrap();
    assert!(manager.live.is_empty());
    manager.uninstall("svg", false).unwrap();
}

/// Quotas apply to the complete split scene; an oversized preview must leave its editor and guest alive.
#[test]
#[ignore = "build markdown through the host SDK first"]
fn markdown_composed_quota_preserves_editor_and_recovers_after_shortening() {
    let package = package("markdown");
    let root = tempfile::tempdir().unwrap();
    let mut manager = manager(root.path());
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let instance = manager.instance_id("markdown").unwrap().to_owned();
    for (revision, text, limited) in [
        (2, "x\n\n".repeat(673), true),
        (3, "# Shorter\n".into(), false),
    ] {
        let source = api::DocumentVersion {
            id: "quota-source".into(),
            path: "quota.md".into(),
            revision,
        };
        notify(
            &mut manager,
            "markdown",
            "preview",
            api::Notification::FilePreview {
                file: Some(api::FileContext {
                    version: api::FileVersion {
                        id: "quota-file".into(),
                        path: source.path.clone(),
                        revision,
                    },
                    file_type: "md".into(),
                    text: Some(source.clone()),
                }),
            },
        );
        notify(
            &mut manager,
            "markdown",
            "preview",
            api::Notification::Preview {
                document: Some(source.clone()),
                text,
            },
        );
        let scene = tree(&manager, "markdown", "preview");
        scene.validate().unwrap();
        assert_eq!(scene.active_node("preview-limit").is_some(), limited);
        assert!(matches!(
            &scene.active_node("markdown-native-editor").unwrap().kind,
            ui::Kind::NativeEditor { document } if document == &source
        ));
        assert_eq!(manager.instance_id("markdown"), Some(instance.as_str()));
        assert!(manager.installed["markdown"].error.is_none());
    }
}
