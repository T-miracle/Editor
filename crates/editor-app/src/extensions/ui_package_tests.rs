//! Real delivered UI packages use the existing worker publication and native document input seams.
use super::composable_tests::{publish, pump};
use super::*;
use gpui_kit::{TestAppContext, gpui};

/// Native clicks, IME-compatible notes and unsaved vectors survive install/revoke without empty regions.
#[gpui::test]
#[ignore = "build example and svg through scripts/build-plugins.ps1 first"]
fn delivered_ui_packages_follow_native_input_and_reclaim_layout(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("drawing.svg");
    let original = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"160\" height=\"100\"/>";
    std::fs::write(&path, original).unwrap();
    let mut manager = plugin_runtime::Manager::open(
        root.path().join("runtime"),
        protocol::Environment {
            workspace: root.path().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    for id in ["example", "svg"] {
        let package = Package::read(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../../dist/plugins/{id}.zip")),
        )
        .unwrap();
        manager
            .install(&package, package.manifest.permissions.clone())
            .unwrap();
    }
    let mut renderer = images::VectorRenderer::default();
    let workspace = Workspace::open(root.path()).unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    cx.simulate_resize(size(px(1400.), px(900.)));
    publish(&mut manager, &mut renderer, &app, cx);
    let button = cx.debug_bounds("plugin-ui-increment").unwrap();
    cx.simulate_click(button.center(), Default::default());
    cx.run_until_parked();
    pump(&mut manager, &app, cx);
    publish(&mut manager, &mut renderer, &app, cx);
    assert!(
        serde_json::to_string(manager.live["example"].views["counter"].as_ref())
            .unwrap()
            .contains("点击次数：1")
    );
    let note = cx.debug_bounds("plugin-ui-note").unwrap();
    cx.simulate_click(note.center(), Default::default());
    cx.simulate_input("中文 notes with spaces");
    cx.run_until_parked();
    pump(&mut manager, &app, cx);
    publish(&mut manager, &mut renderer, &app, cx);
    assert!(
        serde_json::to_string(manager.live["example"].views["notes"].as_ref())
            .unwrap()
            .contains("中文 notes with spaces")
    );
    // Preview contents originate in EditorState memory, with disk left unchanged.
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_file(path.clone(), window, cx)));
    cx.run_until_parked();
    for _ in 0..2 {
        pump(&mut manager, &app, cx);
        publish(&mut manager, &mut renderer, &app, cx);
    }
    assert!(cx.debug_bounds("editor-preview-pane").is_some());
    let canvas = cx.debug_bounds("plugin-ui-preview-canvas").unwrap();
    let drawing = |manager: &plugin_runtime::Manager| {
        let protocol::ui::Kind::Canvas(canvas) =
            &manager.live["svg"].views["preview"].as_ref().root.kind
        else {
            panic!("canvas required")
        };
        canvas
            .paint
            .iter()
            .rev()
            .find_map(|paint| match paint {
                protocol::Paint::Svg { rect, clip, .. } if clip.y > 0. => Some(*rect),
                _ => None,
            })
            .unwrap()
    };
    let before = drawing(&manager);
    cx.simulate_click(canvas.origin + point(px(18.), px(16.)), Default::default());
    cx.run_until_parked();
    pump(&mut manager, &app, cx);
    publish(&mut manager, &mut renderer, &app, cx);
    assert!(drawing(&manager).w > before.w);
    cx.update(|window, cx| {
        app.read(cx)
            .editor
            .clone()
            .update(cx, |editor, cx| editor.focus(window, cx))
    });
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"320\" height=\"100\"><!-- 未保存 --></svg>");
    cx.run_until_parked();
    pump(&mut manager, &app, cx);
    publish(&mut manager, &mut renderer, &app, cx);
    let tree = manager.live["svg"].views["preview"].as_ref();
    assert!(serde_json::to_string(tree).unwrap().contains("未保存"));
    assert!(tree.source.is_some());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    cx.update(|_, cx| {
        let owner = app.read(cx).extensions.read(cx);
        let state = owner.worker.state.lock().unwrap();
        assert!(
            state.images["svg/preview/canvas/preview-canvas"]
                .iter()
                .any(Option::is_some)
        );
    });
    // Host transport limits must produce visible feedback, then recover when memory becomes small again.
    cx.simulate_keystrokes("ctrl-a");
    // One native paste tests the boundary without synthesizing a million individual keystrokes.
    cx.update(|_, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string(
            "<!-- memory -->\n".repeat(70_000),
        ))
    });
    cx.simulate_keystrokes("ctrl-v");
    cx.run_until_parked();
    pump(&mut manager, &app, cx);
    publish(&mut manager, &mut renderer, &app, cx);
    assert!(cx.debug_bounds("plugin-preview-error").is_some());
    assert!(cx.debug_bounds("plugin-ui-preview-canvas").is_none());
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input(original);
    cx.run_until_parked();
    pump(&mut manager, &app, cx);
    publish(&mut manager, &mut renderer, &app, cx);
    assert!(cx.debug_bounds("plugin-preview-error").is_none());
    assert!(cx.debug_bounds("plugin-ui-preview-canvas").is_some());
    assert!(manager.installed["svg"].error.is_none());
    manager.disable("svg").unwrap();
    publish(&mut manager, &mut renderer, &app, cx);
    assert!(cx.debug_bounds("editor-preview-pane").is_none());
    for id in ["svg", "example"] {
        manager.uninstall(id, false).unwrap();
    }
    publish(&mut manager, &mut renderer, &app, cx);
    assert!(cx.debug_bounds("plugin-ui-note").is_none());
    assert!(cx.update(|_, cx| app.read(cx).plugin_panels.is_empty()));
}
