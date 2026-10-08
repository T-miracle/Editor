//! Installed language packages exercise paired edits through real native actions, input and Undo.
use crate::*;
use gpui_kit::{EntityInputHandler as _, TestAppContext, VisualTestContext, gpui};
use plugin_runtime::{Package, plugin_protocol as protocol};
use std::cell::RefCell;
use std::time::{Duration, Instant};

/// The same native interaction scenarios apply to XML and HTML using each package's own parser.
#[gpui::test]
#[ignore = "build the current XML and HTML packages; prepares their approved private native services"]
fn installed_xml_html_tags_share_native_rename_linked_input_and_undo(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::bind_editor_shell_keys(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let xml = "<!-- 中文🙂 -->\n<root xmlns:p=\"urn:example\" xmlns:q=\"urn:renamed\" note=\"中文🙂 p:item\"><p:item><p:item/></p:item><!-- p:item --></root>";
    let html =
        "<!-- 中文🙂 -->\n<main title=\"中文🙂 span\"><span><b></b></span><!-- span --><br></main>";
    for (filename, source) in [("sample.xml", xml), ("sample.html", html)] {
        std::fs::write(directory.path().join(filename), source).unwrap();
    }
    let workspace = Workspace::open(directory.path()).unwrap();
    let mut manager = plugin_runtime::Manager::open(
        workspace.root().join(".runtime-plugin-test"),
        protocol::Environment {
            os: std::env::consts::OS.into(),
            workspace: workspace.root().display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    for id in ["xml", "html"] {
        let package = Package::read(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../../dist/plugins/{id}.zip")),
        )
        .unwrap();
        if id == "xml" {
            super::editing_fixture::install_xml(&mut manager, &package);
        } else {
            manager
                .install(&package, package.manifest.permissions.clone())
                .unwrap();
        }
    }
    let slot = Rc::new(RefCell::new(None));
    let captured = slot.clone();
    let (_, visual) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *captured.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    visual.simulate_resize(size(px(1200.), px(800.)));
    crate::extensions::lsp_tests::publish(&app, &mut manager, visual);
    for (id, source, name) in [("xml", xml, "p:item"), ("html", html, "span")] {
        let path = directory.path().join(format!("sample.{id}"));
        visual.update(|window, cx| {
            window.activate_window();
            app.update(cx, |app, cx| app.open_file(path.clone(), window, cx));
            // Public next-frame dispatch completes the native grammar initialization before input.
            window.draw(cx).clear(cx);
            window.simulate_next_frame(cx);
        });
        let server = visual.update(|_, cx| app.read(cx).language_servers[id].clone());
        let ready = server.prepare_until_ready();
        // Preserve the real service's recovery and stderr tails when setup fails, before any tag assertion.
        assert!(
            ready.is_ok(),
            "{id} startup={ready:?}; recovery={:?}; runtime={:?}",
            server.recovery_status(),
            manager.runtime_logs().records(id)
        );
        // Finish opening and painting the source before the new caret; this does not prewarm its pair.
        visual.run_until_parked();
        visual.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        let start = source.find(&format!("<{name}>")).unwrap() + 1;
        // Move and type within one frame: no sleep or semantic-cache readiness before the first character.
        visual.update(|window, cx| {
            app.read(cx).editor.clone().update(cx, |editor, cx| {
                editor.set_selected_range(start + 1..start + 1, cx);
                editor.focus(window, cx);
                cx.notify();
            });
            window.draw(cx).clear(cx);
        });
        assert!(
            visual.debug_bounds("editor-source-pane").is_some(),
            "native source must be painted"
        );
        visual.update(|window, cx| {
            let editor = app.read(cx).editor.read(cx);
            assert!(
                editor.focus_handle(cx).is_focused(window),
                "native editor must own focus"
            );
            assert!(editor.is_editable(), "native editor must be editable");
        });
        visual.simulate_input("X");
        let changed = source
            .replacen(
                &format!("<{name}>"),
                &format!("<{}X{}>", &name[..1], &name[1..]),
                1,
            )
            .replacen(
                &format!("</{name}>"),
                &format!("</{}X{}>", &name[..1], &name[1..]),
                1,
            );
        await_text(&app, &changed, visual, "immediate linked keystroke");
        visual.simulate_keystrokes("ctrl-z");
        assert_text(&app, source, visual);
        visual.simulate_keystrokes("ctrl-y");
        assert_text(&app, &changed, visual);
        visual.simulate_keystrokes("ctrl-z");
        assert_text(&app, source, visual);
        let lease = server.open_document(crate::language::navigation::file_uri(&path).unwrap());
        let linked = server
            .linked_ranges_for(
                lease,
                source.into(),
                crate::language::navigation::position_at_byte(source, start + 1),
            )
            .unwrap();
        assert!(
            linked
                .as_ref()
                .is_some_and(|linked| linked.response.ranges.len() == 2
                    && linked.response.word_pattern.is_some()),
            "{id} semantic pair: {linked:?}"
        );

        let isolated = if id == "xml" { "p:item/" } else { "br" };
        let isolated_start = source.find(&format!("<{isolated}>")).unwrap() + 1;
        select(&app, isolated_start + 1..isolated_start + 1, visual);
        visual.simulate_input("X");
        assert_text(
            &app,
            &source.replacen(
                &format!("<{isolated}>"),
                &format!("<{}X{}>", &isolated[..1], &isolated[1..]),
                1,
            ),
            visual,
        );
        visual.simulate_keystrokes("ctrl-z");
        assert_text(&app, source, visual);

        // Identical words in comments and attributes are ordinary text, with independent native Undo.
        for ordinary in [
            source.find("中文🙂").unwrap(),
            source.rfind("中文🙂").unwrap(),
        ] {
            select(&app, ordinary..ordinary, visual);
            visual.simulate_input("X");
            assert_text(
                &app,
                &format!("{}X{}", &source[..ordinary], &source[ordinary..]),
                visual,
            );
            visual.simulate_keystrokes("ctrl-z");
            assert_text(&app, source, visual);
        }

        // End-tag paste mirrors only its semantic peer, leaving nested names and comments unchanged.
        let close = source.find(&format!("</{name}>")).unwrap() + 2;
        select(&app, close..close + name.len(), visual);
        visual.update(|_, cx| {
            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("changed".into()))
        });
        visual.simulate_keystrokes("ctrl-v");
        let renamed = source
            .replacen(&format!("<{name}>"), "<changed>", 1)
            .replacen(&format!("</{name}>"), "</changed>", 1);
        assert_text(&app, &renamed, visual);
        visual.simulate_keystrokes("ctrl-z");
        assert_text(&app, source, visual);

        select(&app, start + 2..start + 2, visual);
        visual.simulate_keystrokes("backspace");
        let short = format!("{}{}", &name[..1], &name[2..]);
        let deleted = source
            .replacen(&format!("<{name}>"), &format!("<{short}>"), 1)
            .replacen(&format!("</{name}>"), &format!("</{short}>"), 1);
        assert_text(&app, &deleted, visual);
        visual.simulate_keystrokes("ctrl-z");
        assert_text(&app, source, visual);

        // A full qualified replacement is requested from its local part; plain names use the same native UI.
        let local = name.find(':').map_or(1, |colon| colon + 2);
        let new_name = if id == "xml" { "q:changed" } else { "changed" };
        let explicit = source
            .replacen(&format!("<{name}>"), &format!("<{new_name}>"), 1)
            .replacen(&format!("</{name}>"), &format!("</{new_name}>"), 1);
        rename_at(&app, start + local, new_name, &explicit, visual);
        visual.simulate_keystrokes("ctrl-z");
        assert_text(&app, source, visual);
        // A normal, unqualified element confirms the same behavior without namespace-specific semantics.
        let plain_name = if id == "xml" { "root" } else { "main" };
        let plain_start = source.find(&format!("<{plain_name} ")).unwrap() + 2;
        let plain = source
            .replacen(&format!("<{plain_name} "), "<changed ", 1)
            .replacen(&format!("</{plain_name}>"), "</changed>", 1);
        rename_at(&app, plain_start, "changed", &plain, visual);
        visual.simulate_keystrokes("ctrl-z");
        assert_text(&app, source, visual);

        // Input composition is delivered through the installed native handler bridge, not a second editor.
        // Composition can start and commit before the new caret's linked request has returned.
        visual.update(|window, cx| {
            app.read(cx).editor.clone().update(cx, |editor, cx| {
                editor.set_selected_range(close + 1..close + 1, cx);
                editor.focus(window, cx);
                cx.notify();
            })
        });
        let quick_bridge =
            visual.update(|_, cx| app.read(cx).language_edits.bridge.clone().unwrap());
        visual.update(|window, cx| {
            quick_bridge.update(cx, |bridge, cx| {
                bridge.replace_and_mark_text_in_range(None, "拼", Some(0..1), window, cx)
            })
        });
        visual.update(|window, cx| {
            quick_bridge.update(cx, |bridge, cx| {
                bridge.replace_text_in_range(None, "标签", window, cx)
            })
        });
        let fast_composed = source
            .replacen(
                &format!("<{name}>"),
                &format!("<{}标签{}>", &name[..1], &name[1..]),
                1,
            )
            .replacen(
                &format!("</{name}>"),
                &format!("</{}标签{}>", &name[..1], &name[1..]),
                1,
            );
        await_condition(
            visual,
            |visual| text(&app, visual) == fast_composed,
            "immediate linked composition",
        );
        visual.simulate_keystrokes("ctrl-z");
        assert_text(&app, source, visual);
        select(&app, start + 1..start + 1, visual);
        let bridge = visual.update(|_, cx| app.read(cx).language_edits.bridge.clone().unwrap());
        visual.update(|window, cx| {
            bridge.update(cx, |bridge, cx| {
                bridge.replace_and_mark_text_in_range(None, "拼", Some(0..1), window, cx)
            })
        });
        assert_eq!(
            text(&app, visual),
            source.replacen(
                &format!("<{name}>"),
                &format!("<{}拼{}>", &name[..1], &name[1..]),
                1
            )
        );
        visual.update(|window, cx| {
            bridge.update(cx, |bridge, cx| {
                bridge.replace_text_in_range(None, "标签", window, cx)
            })
        });
        let composed = source
            .replacen(
                &format!("<{name}>"),
                &format!("<{}标签{}>", &name[..1], &name[1..]),
                1,
            )
            .replacen(
                &format!("</{name}>"),
                &format!("</{}标签{}>", &name[..1], &name[1..]),
                1,
            );
        assert_text(&app, &composed, visual);
        visual.simulate_keystrokes("ctrl-z");
        assert_text(&app, source, visual);
        select(&app, start + 1..start + 1, visual);
        visual.update(|window, cx| {
            bridge.update(cx, |bridge, cx| {
                bridge.replace_and_mark_text_in_range(None, "拼", Some(0..1), window, cx)
            })
        });
        visual.update(|window, cx| {
            bridge.update(cx, |bridge, cx| {
                bridge.replace_and_mark_text_in_range(None, "", None, window, cx)
            })
        });
        assert_text(&app, source, visual);

        crate::language::providers::set_editing_preference(id, false, Some(false)).unwrap();
        visual.update(|_, cx| app.update(cx, |app, cx| app.sync_linked_input(cx)));
        select(&app, start + 1..start + 1, visual);
        visual.simulate_input("X");
        assert_text(
            &app,
            &source.replacen(
                &format!("<{name}>"),
                &format!("<{}X{}>", &name[..1], &name[1..]),
                1,
            ),
            visual,
        );
        visual.simulate_keystrokes("ctrl-z");
        assert_text(&app, source, visual);
        crate::language::providers::set_editing_preference(id, false, None).unwrap();
        visual.update(|_, cx| apply_theme(builtin_theme(true), cx));
    }
    manager.disable("html").unwrap();
    crate::extensions::lsp_tests::publish(&app, &mut manager, visual);
    assert!(visual.update(|_, cx| app.read(cx).language_edits.bridge.is_none()));
}

/// Native field copy exposes its entered name; confirmation must change only the expected semantic pair.
fn rename_at(
    app: &Entity<EditorApp>,
    offset: usize,
    name: &str,
    expected: &str,
    visual: &mut VisualTestContext,
) {
    select(app, offset..offset, visual);
    visual.simulate_keystrokes("f2");
    await_condition(
        visual,
        |visual| visual.debug_bounds("editor-rename-prompt").is_some(),
        "rename field",
    );
    visual.simulate_input(name);
    visual.simulate_keystrokes("ctrl-a ctrl-c");
    assert_eq!(
        visual.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text())),
        Some(name.into()),
        "native rename field must own the entered name"
    );
    visual.simulate_keystrokes("enter");
    await_text(app, expected, visual, "paired rename");
}

/// Cursor preparation pumps real transport work after native selection without inspecting bridge internals.
pub(super) fn select(
    app: &Entity<EditorApp>,
    range: std::ops::Range<usize>,
    visual: &mut VisualTestContext,
) {
    visual.update(|window, cx| {
        app.read(cx).editor.clone().update(cx, |editor, cx| {
            editor.set_selected_range(range, cx);
            editor.focus(window, cx);
            cx.notify();
        })
    });
    let deadline = Instant::now() + Duration::from_millis(250);
    while Instant::now() < deadline {
        visual.executor().advance_clock(Duration::from_millis(10));
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        std::thread::sleep(Duration::from_millis(10));
    }
}
pub(super) fn text(app: &Entity<EditorApp>, visual: &mut VisualTestContext) -> String {
    visual.update(|_, cx| app.read(cx).editor.read(cx).text().to_string())
}
pub(super) fn assert_text(app: &Entity<EditorApp>, expected: &str, visual: &mut VisualTestContext) {
    visual.run_until_parked();
    assert_eq!(text(app, visual), expected);
}
/// Failed native edits report the observable document and status, without peeking into bridge caches.
pub(super) fn await_text(
    app: &Entity<EditorApp>,
    expected: &str,
    visual: &mut VisualTestContext,
    label: &str,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        visual.executor().advance_clock(Duration::from_millis(10));
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let current = text(app, visual);
        if current == expected {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "Timed out waiting for {label}: text={current:?}, expected={expected:?}, status={}",
            visual.update(|_, cx| app.read(cx).status.clone())
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
pub(super) fn await_condition(
    visual: &mut VisualTestContext,
    mut condition: impl FnMut(&mut VisualTestContext) -> bool,
    label: &str,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        visual.executor().advance_clock(Duration::from_millis(10));
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        if condition(visual) {
            return;
        }
        assert!(Instant::now() < deadline, "Timed out waiting for {label}");
        std::thread::sleep(Duration::from_millis(10));
    }
}
