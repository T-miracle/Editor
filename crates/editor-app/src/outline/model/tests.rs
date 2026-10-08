//! Native presentation regression after a validated structure snapshot reaches the outline model.
use super::*;
use gpui_kit::{TestAppContext, VisualTestContext, gpui};
use plugin_runtime::plugin_protocol::structure::Proposal;
use std::{cell::RefCell, rc::Rc};

const SOURCE: &str = "<root><first><inner><leaf/></inner></first><second><other/></second></root>";

/// Build nested coverage and precise name ranges without depending on any language parser.
fn node(name: &str, children: Vec<Node>) -> Node {
    let start = SOURCE.find(&format!("<{name}")).unwrap();
    let closing = format!("</{name}>");
    let end = if let Some(end) = SOURCE.find(&closing) {
        end + closing.len()
    } else {
        start + SOURCE[start..].find("/>").unwrap() + 2
    };
    Node {
        name: name.into(),
        kind: "definition".into(),
        icon: None,
        range: TextRange { start, end },
        definition: TextRange {
            start: start + 1,
            end: start + 1 + name.len(),
        },
        children,
    }
}

/// Use native direction keys instead of setting the caret and forcing the owner's render.
fn move_cursor(app: &Entity<EditorApp>, visual: &mut VisualTestContext, name: &str) {
    let offset = SOURCE.find(&format!("<{name}")).unwrap() + 1;
    visual.update(|window, cx| {
        app.read(cx).editor.clone().update(cx, |editor, cx| {
            editor.focus(window, cx);
        });
    });
    visual.simulate_keystrokes("ctrl-home");
    visual.simulate_keystrokes(&vec!["right"; offset].join(" "));
    visual.run_until_parked();
    visual.update(|_, cx| assert_eq!(app.read(cx).editor.read(cx).cursor(), offset));
}

/// Click the painted source caret geometry so mouse navigation follows the same native input path.
fn click_cursor(app: &Entity<EditorApp>, visual: &mut VisualTestContext, name: &str) {
    let offset = SOURCE.find(&format!("<{name}")).unwrap() + 1;
    let position = visual.update(|_, cx| {
        app.read(cx)
            .editor
            .read(cx)
            .range_to_bounds(&(offset..offset))
            .unwrap()
            .center()
    });
    visual.simulate_click(position, Default::default());
    visual.run_until_parked();
    visual.update(|_, cx| assert_eq!(app.read(cx).editor.read(cx).cursor(), offset));
}

/// Distinguish keyboard browsing from document ownership and retract the previous automatic path.
#[gpui::test]
fn native_outline_single_highlight_and_temporary_follow_expansion(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        apply_theme(builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("outline.txt");
    std::fs::write(&path, SOURCE).unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, visual) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
        *capture.borrow_mut() = Some(app.clone());
        Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().unwrap();
    visual.update(|window, cx| {
        window.activate_window();
        app.update(cx, |app, cx| {
            // This presentation fixture explicitly displays the otherwise hidden-by-default tree.
            app.session_state.outline_visible = true;
            app.open_file(path, window, cx);
            app.session_state.outline_follow_cursor = false;
            app.accept_outline(
                StructureSnapshot {
                    proposal: Proposal {
                        request: 1,
                        document: app.plugin_document_version(0).unwrap(),
                        nodes: vec![node(
                            "root",
                            vec![
                                node("first", vec![node("inner", vec![node("leaf", vec![])])]),
                                node("second", vec![node("other", vec![])]),
                            ],
                        )],
                        folds: vec![],
                    },
                    icons: BTreeMap::new(),
                },
                window,
                cx,
            );
        });
    });
    move_cursor(&app, visual, "second");
    // Simulate an independently browsed row while the editor remains inside another definition.
    visual.update(|_, cx| {
        app.read(cx).outline.tree.clone().update(cx, |tree, cx| {
            tree.set_selected_index(tree.index_of(&"/0/0".into()), cx);
        });
    });
    let mut failures = Vec::new();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let bounds = visual.debug_bounds("outline-tree").unwrap();
    let highlights = visual.update(|window, cx| {
        let color = component_styles(cx, ThemeComponent::ExplorerRow)
            .selected
            .background
            .unwrap();
        window
            .painted_quads()
            .iter()
            .filter(|quad| {
                bounds
                    .scale(window.scale_factor())
                    .contains(&quad.bounds.center())
                    && quad.background == color.into()
            })
            .count()
    });
    if highlights != 1 {
        failures.push(format!(
            "expected exactly one cursor-owned background; painted {highlights}"
        ));
    }
    // A restored disabled preference keeps the two-level tree until the model's follow preference is enabled.
    move_cursor(&app, visual, "leaf");
    assert!(visual.debug_bounds("outline-node-/0/0/0/0").is_none());
    // The removed title control is independent of the model's existing workspace follow preference.
    visual.update(|_, cx| {
        app.update(cx, |app, cx| {
            app.session_state.outline_follow_cursor = true;
            cx.notify();
        });
    });
    visual.update(|_, cx| assert!(app.read(cx).session_state.outline_follow_cursor));
    move_cursor(&app, visual, "leaf");
    assert!(visual.debug_bounds("outline-node-/0/0/0/0").is_some());
    click_cursor(&app, visual, "other");
    assert!(visual.debug_bounds("outline-node-/0/1/0").is_some());
    if visual.debug_bounds("outline-node-/0/0/0").is_some() {
        failures.push("the previous automatically expanded branch remained visible".into());
    }
    click_cursor(&app, visual, "second");
    if visual.debug_bounds("outline-node-/0/1/0").is_some() {
        failures.push("moving back to level two did not collapse deeper rows".into());
    }
    assert!(failures.is_empty(), "{}", failures.join("; "));
}
