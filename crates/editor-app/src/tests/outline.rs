//! Actual XML and unfamiliar-ID packages enter public Manager before native outline and dock interaction.
use crate::*;
use gpui_kit::{TestAppContext, VisualTestContext, gpui};
use std::{
    cell::RefCell,
    io::{Cursor, Write},
    time::{Duration, Instant},
};

/// Repackage only declarations: the actual independent guest/grammar remain the product's distributed bytes.
fn package(novel: bool) -> plugin_runtime::Package {
    let original = plugin_runtime::Package::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/xml.zip"),
    )
    .unwrap();
    let mut manifest = serde_json::to_value(&original.manifest).unwrap();
    manifest["language_servers"] = serde_json::json!([]);
    manifest["services"] = serde_json::json!({});
    manifest["permissions"] = serde_json::json!(["editor.read"]);
    manifest["api"]["required"] =
        serde_json::json!({"configuration":"^1", "language.structure":"^1"});
    manifest["structure_providers"] =
        serde_json::json!([{"id":"structure","language":if novel {"novel-tree"} else {"xml"}}]);
    let mut files = original.files;
    if novel {
        manifest["id"] = serde_json::json!("novel-tree");
        // Only identities change: the fixture still recognizes .xml and the real WASM export remains tree_sitter_xml.
        let declaration = String::from_utf8(files["plugin.toml"].clone())
            .unwrap()
            .replace("id = \"xml\"", "id = \"novel-tree\"")
            .replace("language = \"xml\"", "language = \"novel-tree\"");
        files.insert("plugin.toml".into(), declaration.into_bytes());
        // The unfamiliar package also paints and navigates through the native definition-icon fallback.
        files.remove("icons/element.svg");
    }
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
    plugin_runtime::Package::from_bytes(&archive.finish().unwrap().into_inner()).unwrap()
}

use crate::extensions::lsp_tests::publish;
mod docking;
mod folding;
mod lifetime;

/// Advance the real native frame/tasks until the selected package's tree is painted.
fn wait_outline(cx: &mut VisualTestContext) {
    wait_node(cx, "outline-node-/0/0");
}

/// Observe a genuinely painted virtual row, including automatic scrolling to a distant definition.
fn wait_node(cx: &mut VisualTestContext, selector: &'static str) {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        // Base debounce and asynchronous highlighting use the test dispatcher clock, while WASM still uses wall time.
        cx.executor().advance_clock(Duration::from_millis(10));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        if cx.debug_bounds(selector).is_some() {
            break;
        }
        if Instant::now() >= deadline {
            let roles = language::providers::rows()
                .into_iter()
                .map(|row| (row.key, row.selected, row.candidates))
                .collect::<Vec<_>>();
            panic!(
                "actual package failed to paint {selector}; root={:?}; empty={:?}; loading={:?}; failed={:?}; roles={roles:?}",
                cx.debug_bounds("outline-node-/0"),
                cx.debug_bounds("outline-empty"),
                cx.debug_bounds("outline-loading"),
                cx.debug_bounds("outline-failed")
            );
        }
        std::thread::sleep(Duration::from_millis(15));
    }
}

/// Tree clicks, keyboard focus, optional cursor following, unsaved refresh and revocation share one native editor.
#[gpui::test]
#[ignore = "build actual XML ZIP with the current public SDK first"]
fn native_outline_package_navigation_follow_and_revocation(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
    });
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tree.xml");
    let source = "<root>\n  <!-- 多行🙂\n       comment body\n       end -->\n  <item id=\"中文🙂\">\n    <leaf name=\"child\"/>\n  </item>\n</root>";
    std::fs::write(&path, source).unwrap();
    std::fs::write(directory.path().join("empty.txt"), "plain").unwrap();
    let package = package(true);
    let workspace = Workspace::open(directory.path()).unwrap();
    // Match the existing cfg(test) extension root so real package contributions load through the production reader.
    let mut manager = plugin_runtime::Manager::open(
        workspace.root().join(".runtime-plugin-test"),
        plugin_runtime::plugin_protocol::Environment {
            workspace: workspace.root().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, visual) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    publish(&app, &mut manager, visual);
    visual.update(|window, cx| {
        window.activate_window();
        app.update(cx, |app, cx| app.open_file(path.clone(), window, cx));
    });
    wait_outline(visual);
    visual.update(|window, cx| {
        let focus = app.read(cx).outline_panel.focus_handle(cx);
        focus.focus(window, cx);
    });
    visual.run_until_parked();
    visual.simulate_keystrokes("left");
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        visual.debug_bounds("outline-node-/0/0").is_none(),
        "Dock focus must enter Base's tree direction-key behavior"
    );
    visual.simulate_keystrokes("right");
    wait_outline(visual);
    let child = visual.debug_bounds("outline-node-/0/0").unwrap();
    visual.simulate_click(child.center(), Default::default());
    visual.run_until_parked();
    visual.update(|window, cx| {
        let app = app.read(cx);
        let editor = app.editor.read(cx);
        assert_eq!(
            editor.cursor(),
            source.find("item id").unwrap(),
            "jump must target the opening name, not coverage start"
        );
        assert!(app.editor.focus_handle(cx).is_focused(window));
    });
    // Click a real comment gutter fold using laid-out source geometry, including the definition jump's scroll.
    assert!(
        folding::fold_at(&app, 1, 2, visual),
        "comment structure must expose a functioning native gutter fold"
    );
    visual.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.editor.update(cx, |editor, cx| {
                editor.set_cursor_position(lsp_types::Position::new(5, 6), window, cx)
            });
            cx.notify();
        })
    });
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        visual.debug_bounds("outline-node-/0/0/0").is_some(),
        "enabled follow expands the innermost element's ancestors"
    );
    // A disclosure changes expansion without moving the editor or activating its definition.
    let before_disclosure = visual.update(|_, cx| app.read(cx).editor.read(cx).cursor());
    // Follow-disabled browsing retains a collapsed branch while the native editor moves inside it.
    visual.update(|_, cx| {
        app.update(cx, |app, cx| {
            app.session_state.outline_follow_cursor = false;
            cx.notify();
        })
    });
    let child = visual.debug_bounds("outline-node-/0/0").unwrap();
    visual.simulate_click(
        point(child.left() + px(28.), child.center().y),
        Default::default(),
    );
    visual.update(|_, cx| assert_eq!(app.read(cx).editor.read(cx).cursor(), before_disclosure));
    visual.simulate_keystrokes("right");
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        visual.debug_bounds("outline-node-/0/0/0").is_some(),
        "Base right arrow expands the selected folder"
    );
    visual.simulate_keystrokes("down");
    visual.simulate_keystrokes("enter");
    visual.run_until_parked();
    visual.update(|window, cx| {
        let app = app.read(cx);
        assert_eq!(
            app.editor.read(cx).cursor(),
            source.find("leaf name").unwrap(),
            "Enter navigates to the selected leaf"
        );
        assert!(app.editor.focus_handle(cx).is_focused(window));
    });
    // Clicking the leaf's blank disclosure slot performs normal navigation; it cannot hide another node.
    let leaf = visual.debug_bounds("outline-node-/0/0/0").unwrap();
    visual.simulate_click(
        point(leaf.left() + px(42.), leaf.center().y),
        Default::default(),
    );
    visual.run_until_parked();
    assert!(visual.debug_bounds("outline-node-/0/0/0").is_some());
    let child = visual.debug_bounds("outline-node-/0/0").unwrap();
    visual.simulate_click(
        point(child.left() + px(28.), child.center().y),
        Default::default(),
    );
    visual.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.editor.update(cx, |editor, cx| {
                editor.set_cursor_position(lsp_types::Position::new(5, 8), window, cx)
            });
            cx.notify();
        })
    });
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    visual.update(|_, cx| {
        let app = app.read(cx);
        let tree = app.outline.tree.read(cx);
        let index = tree.index_of(&"/0/0".into()).unwrap();
        assert!(
            !tree.entry(index).unwrap().is_expanded(),
            "disabled follow must not force ancestor expansion"
        );
    });
    // Preserve a deliberately expanded child across multiple native revisions while the first structure job is pending.
    let child = visual.debug_bounds("outline-node-/0/0").unwrap();
    visual.simulate_click(
        point(child.left() + px(28.), child.center().y),
        Default::default(),
    );
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("outline-node-/0/0/0").is_some());
    // Unsaved native Unicode input updates labels without touching the disk or reopening the document.
    let insert = source.find("中文🙂").unwrap();
    visual.update(|window, cx| {
        app.read(cx).editor.clone().update(cx, |editor, cx| {
            editor.set_selected_range(insert..insert, cx);
            editor.focus(window, cx);
        })
    });
    for character in ["新", "续"] {
        // Dispatch genuine platform text but intentionally leave request completion unpumped until both revisions exist.
        visual.update(|window, cx| {
            window.dispatch_keystroke(
                gpui_kit::Keystroke {
                    modifiers: Default::default(),
                    key: character.into(),
                    key_char: Some(character.into()),
                },
                cx,
            );
        });
        visual.update(|window, cx| {
            app.update(cx, |app, cx| app.sync_outline(window, cx));
            window.draw(cx).clear(cx);
        });
        assert!(visual.debug_bounds("outline-loading").is_some());
    }
    wait_outline(visual);
    visual.update(|_, cx| {
        let tree = app.read(cx).outline.tree.read(cx);
        let index = tree.index_of(&"/0/0".into()).unwrap();
        assert!(
            tree.entry(index)
                .unwrap()
                .item()
                .label
                .contains("新续中文🙂")
        );
        assert!(
            tree.entry(index).unwrap().is_expanded(),
            "a revision refresh keeps manual expansion"
        );
    });
    assert_eq!(std::fs::read_to_string(&path).unwrap(), source);
    visual.simulate_keystrokes("ctrl-z");
    wait_outline(visual);
    visual.update(|_, cx| assert_eq!(app.read(cx).editor.read(cx).text().to_string(), source));
    visual.update(|_, cx| apply_theme(builtin_theme(true), cx));
    visual.simulate_scale_factor_change(2.);
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("outline-node-/0/0").is_some());
    // Native paste creates one Undo step and exercises Base virtualization/follow scrolling on an unsaved document.
    let long_source = format!(
        "<root>\n{}</root>",
        (0..90)
            .map(|index| format!("  <node id=\"{index}\"/>\n"))
            .collect::<String>()
    );
    visual.update(|window, cx| {
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(long_source.clone()));
        app.read(cx).editor.clone().update(cx, |editor, cx| {
            editor.set_selected_range(0..editor.text().len(), cx);
            editor.focus(window, cx);
        })
    });
    visual.simulate_keystrokes("ctrl-v");
    wait_outline(visual);
    visual.update(|_, cx| assert_eq!(app.read(cx).editor.read(cx).text().to_string(), long_source));
    visual.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.session_state.outline_follow_cursor = true;
            app.editor.update(cx, |editor, cx| {
                editor.set_cursor_position(lsp_types::Position::new(76, 5), window, cx)
            });
            cx.notify();
        })
    });
    wait_node(visual, "outline-node-/0/75");
    assert!(
        visual.debug_bounds("outline-node-/0/0").is_none(),
        "following a distant definition must scroll its virtual tree"
    );
    let distant = visual.debug_bounds("outline-node-/0/75").unwrap();
    visual.simulate_click(distant.center(), Default::default());
    visual.update(|_, cx| {
        assert_eq!(
            app.read(cx).editor.read(cx).cursor(),
            long_source.find("node id=\"75\"").unwrap()
        )
    });
    visual.update(|_, cx| {
        app.update(cx, |app, cx| {
            app.session_state.outline_follow_cursor = false;
            cx.notify();
        })
    });
    let tree_bounds = visual.debug_bounds("outline-tree").unwrap();
    visual.simulate_event(gpui_kit::ScrollWheelEvent {
        position: tree_bounds.center(),
        delta: gpui_kit::ScrollDelta::Pixels(point(px(0.), px(5000.))),
        ..Default::default()
    });
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        visual.debug_bounds("outline-node-/0/0").is_some(),
        "native wheel browsing must remain at its own scroll position with follow disabled"
    );
    visual.simulate_scale_factor_change(1.);
    visual.simulate_keystrokes("ctrl-z");
    wait_outline(visual);
    visual.update(|_, cx| assert_eq!(app.read(cx).editor.read(cx).text().to_string(), source));
    visual.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.open_file(path.parent().unwrap().join("empty.txt"), window, cx)
        })
    });
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("outline-empty").is_some());
    assert!(visual.debug_bounds("outline-node-/0").is_none());
    visual.update(|window, cx| app.update(cx, |app, cx| app.open_file(path.clone(), window, cx)));
    wait_outline(visual);
    let closed_path = path.canonicalize().unwrap();
    visual.update(|window, cx| {
        app.update(cx, |app, cx| {
            // Existing close policy requires a saved tab, even when native Undo restored its original bytes.
            app.save_current(cx);
            app.close_tab(closed_path.clone(), window, cx);
        })
    });
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    visual.update(|_, cx| {
        assert!(
            app.read(cx)
                .tabs
                .iter()
                .all(|tab| tab.path() != closed_path)
        )
    });
    assert!(visual.debug_bounds("outline-node-/0").is_none());
    visual.update(|window, cx| app.update(cx, |app, cx| app.open_file(path.clone(), window, cx)));
    wait_outline(visual);
    manager.disable("novel-tree").unwrap();
    publish(&app, &mut manager, visual);
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("outline-empty").is_some());
    assert!(
        visual.debug_bounds("outline-node-/0").is_none(),
        "retired leases cannot paint an old document tree"
    );
}
