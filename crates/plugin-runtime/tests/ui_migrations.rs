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
    assert_eq!(package.manifest.permissions, ["editor.read".into()].into());
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
    notify(
        &mut manager,
        "svg",
        "preview",
        api::Notification::Preview {
            document: Some(version.clone()),
            text: SVG.into(),
        },
    );
    let view = tree(&manager, "svg", "preview");
    assert_eq!(view.source.as_ref(), Some(&version));
    let vector = |manager: &Manager| {
        let ui::Kind::Canvas(canvas) = &tree(manager, "svg", "preview").root.kind else {
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
    notify(
        &mut manager,
        "svg",
        "preview",
        api::Notification::Preview {
            document: Some(reopened.clone()),
            text: SVG.into(),
        },
    );
    assert_eq!(
        tree(&manager, "svg", "preview").source.as_ref(),
        Some(&reopened)
    );
    assert_eq!(vector(&manager).w, initial.w);
    notify(
        &mut manager,
        "svg",
        "preview",
        api::Notification::Preview {
            document: None,
            text: String::new(),
        },
    );
    assert!(tree(&manager, "svg", "preview").source.is_none());
    manager.disable("svg").unwrap();
    assert!(manager.live.is_empty());
    manager.uninstall("svg", false).unwrap();
}
