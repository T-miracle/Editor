//! Structure folds own only their native document; untouched Rust grammar folds survive switching and XML revocation.
use super::*;
use gpui_base::input::RopeExt as _;

/// Click native gutter geometry and observe public unfold_at, which returns true only when an interior was actually hidden.
pub(super) fn fold_at(
    app: &Entity<EditorApp>,
    first: u32,
    hidden: u32,
    visual: &mut VisualTestContext,
) -> bool {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        // A newly mounted grammar can require Base's 150ms debounce before native candidates exist.
        // Several real draws per attempt are costly; ensure the virtual 150ms timer advances within the wall bound too.
        visual.executor().advance_clock(Duration::from_millis(50));
        visual.run_until_parked();
        visual.update(|window, cx| {
            // Test windows have no platform frame loop; run the production language initialization scheduled on the next frame.
            window.simulate_next_frame(cx);
            window.draw(cx).clear(cx);
        });
        let (row, horizontal_scroll) = visual.update(|_, cx| {
            let editor = app.read(cx).editor.read(cx);
            let offset = editor.text().line_start_offset(first as usize);
            (
                editor.range_to_bounds(&(offset..offset)).unwrap(),
                editor.scroll_offset().x,
            )
        });
        for offset in [8., 12., 16., 20., 24., 28.] {
            // The first source caret includes the gutter and actual vertical scroll; gutter chevrons paint after hover.
            let position = point(row.left() - horizontal_scroll - px(offset), row.center().y);
            visual.simulate_mouse_move(position, None, Default::default());
            visual.update(|window, cx| window.draw(cx).clear(cx));
            visual.simulate_click(position, Default::default());
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
            let (opened, header_opened) = visual.update(|_, cx| {
                app.read(cx).editor.clone().update(cx, |editor, cx| {
                    // An accidental outer fold hides this header; opening the header first must therefore be a no-op.
                    let header_opened = editor.unfold_at(lsp_types::Position::new(first, 0), cx);
                    (
                        editor.unfold_at(lsp_types::Position::new(hidden, 0), cx),
                        header_opened,
                    )
                })
            });
            if opened && !header_opened {
                return true;
            }
        }
        if Instant::now() >= deadline {
            // Diagnose the ordinary public parser path only after an actual gutter attempt failed, without supplying folds.
            let diagnostic = visual.update(|_, cx| {
                let app = app.read(cx);
                let editor = app.editor.read(cx);
                let language = editor.language_name();
                let ready = app
                    .dynamic_languages
                    .entries
                    .iter()
                    .map(|(provider, state)| (provider.declaration.language.clone(), state.clone()))
                    .collect::<Vec<_>>();
                let mut syntax =
                    gpui_kit::component::highlighter::SyntaxHighlighter::new(&language);
                let complete = syntax.update(None, editor.text(), None);
                (
                    language,
                    ready,
                    complete,
                    syntax.language().clone(),
                    syntax.tree().map(|tree| tree.root_node().to_sexp()),
                    editor.visible_row_range(),
                )
            });
            eprintln!("native gutter failed at {first}->{hidden}; ordinary parser: {diagnostic:?}");
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A grammar refresh after an immutable structure job must not overwrite its revision's fold contribution.
#[gpui::test]
#[ignore = "build actual XML ZIP with the current public SDK first"]
fn native_structure_folds_refresh_without_clearing_unrelated_rust(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let xml_path = directory.path().join("tree.xml");
    let rust_path = directory.path().join("main.rs");
    let xml = "<root>\n<!-- 多行注释🙂\nbody\nend -->\n<item>\n<leaf/>\n</item>\n</root>";
    let rust = "fn main() {\n    let message = \"中文🙂\";\n    if true {\n        println!(\"{message}\");\n    }\n}";
    std::fs::write(&xml_path, xml).unwrap();
    std::fs::write(&rust_path, rust).unwrap();
    let package = package(false);
    let rust_package = crate::extensions::language_tests::packages::rust_resource_package();
    let workspace = Workspace::open(directory.path()).unwrap();
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
    manager
        .install(&rust_package, rust_package.manifest.permissions.clone())
        .unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, visual) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    visual.simulate_resize(size(px(1200.), px(820.)));
    publish(&app, &mut manager, visual);
    // Establish the unchanged ordinary Rust adapter before this application has ever opened a structure document.
    visual.update(|window, cx| {
        app.update(cx, |app, cx| app.open_file(rust_path.clone(), window, cx))
    });
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        fold_at(&app, 0, 2, visual),
        "ordinary Rust grammar folding must exist before any XML structure is attached"
    );
    visual
        .update(|window, cx| app.update(cx, |app, cx| app.open_file(xml_path.clone(), window, cx)));
    wait_outline(visual);
    assert!(
        fold_at(&app, 1, 2, visual),
        "XML comment must be a provider-confirmed native fold"
    );
    visual.update(|window, cx| {
        app.read(cx).editor.clone().update(cx, |editor, cx| {
            editor.set_selected_range(xml.find("body").unwrap()..xml.find("body").unwrap(), cx);
            editor.focus(window, cx);
        })
    });
    // Native multi-line input creates a new revision and changes the comment's complete coverage.
    visual.simulate_input("新的行\n");
    wait_outline(visual);
    let until = Instant::now() + Duration::from_millis(300);
    while Instant::now() < until {
        visual.executor().advance_clock(Duration::from_millis(10));
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        fold_at(&app, 1, 3, visual),
        "same-revision grammar completion cannot erase the refreshed structure fold"
    );
    // This Rust entity never receives structure: its original grammar adapter and native candidates remain intact.
    visual.update(|window, cx| {
        app.update(cx, |app, cx| app.open_file(rust_path.clone(), window, cx))
    });
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("outline-empty").is_some());
    assert!(
        fold_at(&app, 0, 2, visual),
        "switching away from XML must retain Rust grammar folding"
    );
    manager.disable("xml").unwrap();
    publish(&app, &mut manager, visual);
    assert!(
        fold_at(&app, 0, 2, visual),
        "revoking XML while Rust is active must not clear Rust candidates"
    );
    visual
        .update(|window, cx| app.update(cx, |app, cx| app.open_file(xml_path.clone(), window, cx)));
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("outline-node-/0").is_none());
    assert!(visual.debug_bounds("outline-empty").is_some());
}
