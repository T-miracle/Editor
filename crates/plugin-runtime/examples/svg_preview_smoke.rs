//! Verify the actual SVG WASM package through installation and public runtime messages.

use plugin_runtime::{
    Manager, Package,
    plugin_protocol::{Environment, FontStyle, Paint, Rect, api, ui},
};
use std::path::Path;

/// Deliver native surface events with the same identity used by the editor's inner split.
fn send(manager: &mut Manager, event: api::Notification) -> anyhow::Result<()> {
    manager.event("svg", Some("preview".into()), event)
}

/// Read the ordinary canvas node without relying on a legacy scene drawing slot.
fn drawing(manager: &Manager) -> &ui::Canvas {
    let ui::Kind::Canvas(canvas) = &manager.live["svg"].views["preview"].as_ref().root.kind else {
        panic!("canvas required")
    };
    canvas
}
/// Native canvas input uses the revision published by the plugin's current tree.
fn canvas(manager: &mut Manager, action: ui::CanvasEvent) -> anyhow::Result<()> {
    let revision = manager.live["svg"].views["preview"].as_ref().revision;
    send(
        manager,
        api::Notification::Ui(ui::UiEvent {
            revision,
            node: "preview-canvas".into(),
            action: ui::Action::Canvas(action),
        }),
    )
}

/// Observe the public vector operation rather than guest-private view state.
fn vector(manager: &Manager) -> Option<Rect> {
    drawing(manager).paint.iter().find_map(|operation| {
        // Toolbar SVGs use a clip starting at zero; only document vectors occupy the viewport.
        if let Paint::Svg { rect, clip, .. } = operation
            && clip.y > 0.
        {
            Some(*rect)
        } else {
            None
        }
    })
}

/// The 600×400 fixture has a 32px toolbar; every zoom must preserve its viewport center.
fn assert_centered(manager: &Manager) {
    let rect = vector(manager).expect("document vector");
    assert!((rect.x + rect.w / 2. - 300.).abs() < 0.01);
    assert!((rect.y + rect.h / 2. - 216.).abs() < 0.01);
}

/// Real component dispatch must retain alpha-bearing source, survive edits, and obey permission checks.
fn main() -> anyhow::Result<()> {
    let arguments = std::env::args().collect::<Vec<_>>();
    let package = Package::read(Path::new(arguments.get(1).expect("path to svg.zip")))?;
    let directory = tempfile::tempdir()?;
    let root = arguments
        .get(2)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| directory.path().into());
    if arguments.get(2).is_some() {
        // Persistent fixtures are reserved for the native startup script and must stay in target/.
        std::fs::create_dir_all(&root)?;
        anyhow::ensure!(
            root.canonicalize()?
                .starts_with(std::env::current_dir()?.join("target").canonicalize()?),
            "Fixture roots must stay under target/"
        );
    }
    let environment = Environment {
        workspace: arguments.get(3).cloned().unwrap_or_default(),
        ui_font: FontStyle {
            family: Some("Segoe UI".into()),
            size_px: Some(16.),
            bold: Some(false),
        },
        ..Default::default()
    };
    let mut manager = Manager::open(root.clone(), environment.clone())?;
    manager.install(&package, package.manifest.permissions.clone())?;
    if arguments.get(2).is_none() {
        // Replay a legacy installation only inside this owned temporary fixture.
        drop(manager);
        let mut installed = Manager::read_registry(&root)?;
        let mut entry = installed.remove("svg").expect("installed SVG");
        entry.manifest.id = "me.svg-preview".into();
        entry.manifest.name = "SVG Preview".into();
        // Only genuinely retired records participate in historical identity migration.
        entry.manifest.protocol = 6;
        std::fs::rename(
            root.join("packages/svg"),
            root.join("packages/me.svg-preview"),
        )?;
        if root.join("data/svg").exists() {
            std::fs::rename(root.join("data/svg"), root.join("data/me.svg-preview"))?;
        }
        installed.insert("me.svg-preview".into(), entry);
        std::fs::write(root.join("registry.json"), serde_json::to_vec(&installed)?)?;
        manager = Manager::open(root.clone(), environment)?;
        assert_eq!(manager.installed["svg"].manifest.name, "SVG");
        assert_eq!(
            manager.installed["svg"].grants,
            package.manifest.permissions
        );
        assert!(root.join("registry.before-plugin-id-rename.json").is_file());
        assert!(root.join("packages/me.svg-preview").is_dir());
        assert!(!manager.live.contains_key("svg"));
        manager.install(&package, package.manifest.permissions.clone())?;
        assert!(manager.live.contains_key("svg"));
    }
    canvas(
        &mut manager,
        ui::CanvasEvent::Resize {
            width: 600.,
            height: 400.,
            grid: None,
        },
    )?;
    let source = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 200 100\"><circle cx=\"100\" cy=\"50\" r=\"40\" fill=\"#667180\" fill-opacity=\"0.5\"/></svg>";
    send(
        &mut manager,
        api::Notification::Preview {
            document: Some(api::DocumentVersion {
                id: "draft.svg".into(),
                path: "draft.svg".into(),
                revision: 1,
            }),
            text: source.into(),
        },
    )?;
    let original = vector(&manager).expect("valid SVG scene");
    assert!((original.w - 240.).abs() < 0.01);
    assert!((original.h - 120.).abs() < 0.01);
    assert_centered(&manager);
    let scene = drawing(&manager);
    assert!(
        matches!(scene.paint.last(), Some(Paint::Svg { source: rendered, .. }) if rendered == source)
    );
    assert!(scene.paint.iter().any(|paint| matches!(paint,
        Paint::Text { text, size, font, .. } if text == "120%" && *size == 16.
            && font.as_deref().or(scene.font.family.as_deref()).unwrap() == "Segoe UI"
    )));
    assert_eq!(
        scene
            .paint
            .iter()
            .filter(|paint| matches!(paint, Paint::Svg { clip, .. } if clip.y == 0.))
            .count(),
        4
    );
    // Route physical toolbar clicks through the same pointer events emitted by the native surface.
    for (x, expected_width) in [(18., 268.8), (50., 240.), (82., 200.), (114., 552.)] {
        for phase in [ui::PointerPhase::Down, ui::PointerPhase::Up] {
            canvas(
                &mut manager,
                ui::CanvasEvent::Pointer {
                    phase,
                    x,
                    y: 16.,
                    button: 0,
                    clicks: 1,
                    shift: false,
                },
            )?;
        }
        assert!((vector(&manager).unwrap().w - expected_width).abs() < 0.01);
        assert_centered(&manager);
    }
    // A new document resets the default size after the window-fit action.
    send(
        &mut manager,
        api::Notification::Preview {
            document: Some(api::DocumentVersion {
                id: "another-draft.svg".into(),
                path: "another-draft.svg".into(),
                revision: 1,
            }),
            text: source.into(),
        },
    )?;
    canvas(
        &mut manager,
        ui::CanvasEvent::Wheel {
            delta_x: 0.,
            delta_y: 1. * 16.,
            shift: false,
            x: 550.,
            y: 360.,
        },
    )?;
    let zoomed = vector(&manager).unwrap();
    assert!((zoomed.w - 268.8).abs() < 0.01);
    assert_centered(&manager);
    canvas(
        &mut manager,
        ui::CanvasEvent::Wheel {
            delta_x: 0.,
            delta_y: -100. * 16.,
            shift: false,
            x: 300.,
            y: 200.,
        },
    )?;
    assert!((vector(&manager).unwrap().w - 2.).abs() < 0.01);
    assert_centered(&manager);
    canvas(
        &mut manager,
        ui::CanvasEvent::Wheel {
            delta_x: 0.,
            delta_y: 100. * 16.,
            shift: false,
            x: 300.,
            y: 200.,
        },
    )?;
    assert!((vector(&manager).unwrap().w - 6400.).abs() < 0.01);
    assert_centered(&manager);
    send(
        &mut manager,
        api::Notification::Preview {
            document: Some(api::DocumentVersion {
                id: "draft.svg".into(),
                path: "draft.svg".into(),
                revision: 1,
            }),
            text: "<svg".into(),
        },
    )?;
    assert!(vector(&manager).is_none());
    assert!(manager.installed["svg"].error.is_none());
    send(
        &mut manager,
        api::Notification::Preview {
            document: Some(api::DocumentVersion {
                id: "draft.svg".into(),
                path: "draft.svg".into(),
                revision: 1,
            }),
            text: source.into(),
        },
    )?;
    assert!(vector(&manager).is_some());
    send(
        &mut manager,
        api::Notification::Preview {
            document: None,
            text: String::new(),
        },
    )?;
    assert!(vector(&manager).is_none());
    // Reject permission revocation before entering the guest or destroying its live instance.
    manager.installed.get_mut("svg").unwrap().grants.clear();
    assert!(
        send(
            &mut manager,
            api::Notification::Preview {
                document: Some(api::DocumentVersion {
                    id: "draft.svg".into(),
                    path: "draft.svg".into(),
                    revision: 1
                }),
                text: source.into()
            }
        )
        .is_err()
    );
    assert!(manager.live.contains_key("svg"));
    manager.installed.get_mut("svg").unwrap().grants = package.manifest.permissions.clone();
    manager.install(&package, package.manifest.permissions.clone())?;
    send(
        &mut manager,
        api::Notification::Preview {
            document: Some(api::DocumentVersion {
                id: "draft.svg".into(),
                path: "draft.svg".into(),
                revision: 1,
            }),
            text: source.into(),
        },
    )?;
    assert!(vector(&manager).is_some());
    println!(
        "PASS: real Image WASM, 240px minimum SVG default, centered wheel/button zoom, editor typography, SVG toolbar, layers, edit recovery and hot update"
    );
    Ok(())
}
