//! Exercises definition navigation through the same mouse and host callbacks as the editor.

use crate::*;
use editor_core::Workspace;
use gpui_base::input::DefinitionProvider;
use gpui_kit::{TestAppContext, component::Root, gpui, size};
use lsp_types::{LocationLink, Position};
use std::{cell::RefCell, ops::Range, rc::Rc, str::FromStr as _};

/// Return a known definition so mouse dispatch and document reveal remain deterministic.
struct ReadyDefinition {
    location: LocationLink,
    source: Range<usize>,
}

impl DefinitionProvider for ReadyDefinition {
    /// Check that hit testing queried the clicked identifier before supplying its target.
    fn definitions(
        &self,
        _: &gpui_base::input::Rope,
        offset: usize,
        _: &mut Window,
        _: &mut App,
    ) -> gpui_kit::Task<anyhow::Result<Vec<LocationLink>>> {
        assert!(
            self.source.contains(&offset),
            "unexpected query offset {offset}"
        );
        gpui_kit::Task::ready(Ok(vec![self.location.clone()]))
    }
}

/// Middle-click must reveal SideTab's definition, including after the reveal frames settle.
#[gpui::test]
fn middle_click_definition_stays_on_target(cx: &mut TestAppContext) {
    exercise_definition_click(cx, MouseButton::Middle, false, false);
}

/// Ctrl-click invokes showDocument while the source editor is leased by mouse dispatch.
#[gpui::test]
fn ctrl_click_definition_releases_source_editor(cx: &mut TestAppContext) {
    exercise_definition_click(cx, MouseButton::Left, true, false);
}

/// Jumping from SideTabs' own declaration still goes through the host without reentry.
#[gpui::test]
fn ctrl_click_definition_on_declaration(cx: &mut TestAppContext) {
    exercise_definition_click(cx, MouseButton::Left, true, true);
}

/// Use the reported protocol file while replacing only the language server response.
fn exercise_definition_click(
    cx: &mut TestAppContext,
    button: MouseButton,
    ctrl: bool,
    declaration: bool,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        theme::apply_theme(theme::builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("controls.rs");
    let content = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../plugin-protocol/src/ui/controls.rs"
    ));
    std::fs::write(&path, content).unwrap();
    let path = path.canonicalize().unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let initial_path = path.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| EditorApp::new(workspace, Some(initial_path), window, cx));
        *capture.borrow_mut() = Some(view.clone());
        Root::new(view, window, cx)
    });
    let view = slot.borrow_mut().take().unwrap();
    cx.simulate_resize(size(px(1100.), px(700.)));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let (source, target, expected) = cx.update(|_, cx| {
        let app = view.read(cx);
        let text = app.editor.read(cx).text();
        let (source_start, target_start, length) = if declaration {
            let start = content.find("pub struct SideTabs {").unwrap() + 11;
            (start, start, "SideTabs".len())
        } else {
            (
                content.find("Vec<SideTab>").unwrap() + 4,
                content.find("pub struct SideTab {").unwrap() + 11,
                "SideTab".len(),
            )
        };
        (
            source_start..source_start + length,
            target_start..target_start + length,
            text.offset_to_position(target_start),
        )
    });
    // Install the production host callback without starting a real language server.
    let (_manager, plan) = crate::tests::declared_language_service(directory.path());
    let server = Arc::new(language_navigation::LanguageServer::from_service(plan).unwrap());
    let uri = lsp_types::Uri::from_str(url::Url::from_file_path(&path).unwrap().as_str()).unwrap();
    cx.update(|_, cx| {
        view.update(cx, |app, cx| {
            let editor = app.editor.clone();
            editor::attach_language_server(&editor, &path, server, cx.entity().downgrade(), cx);
            editor.update(cx, |state, _| {
                state.lsp_mut().definition_provider = Some(Rc::new(ReadyDefinition {
                    location: LocationLink {
                        origin_selection_range: None,
                        target_uri: uri,
                        target_range: lsp_types::Range::new(
                            expected,
                            Position::new(expected.line, expected.character + target.len() as u32),
                        ),
                        target_selection_range: lsp_types::Range::new(
                            expected,
                            Position::new(expected.line, expected.character + target.len() as u32),
                        ),
                    },
                    source: source.clone(),
                }));
                state.lsp_mut().hover_provider = None;
                state.lsp_mut().completion_provider = None;
            });
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let position = cx.update(|_, cx| {
        view.read(cx)
            .editor
            .read(cx)
            .range_to_bounds(&source)
            .unwrap()
            .center()
    });
    let modifiers = Modifiers {
        control: ctrl,
        ..Default::default()
    };
    // Ctrl-hover populates the engine's cached definition before its click handler runs.
    cx.simulate_mouse_move(position, None::<MouseButton>, modifiers);
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.simulate_mouse_down(position, button, modifiers);
    cx.simulate_mouse_up(position, button, modifiers);
    cx.run_until_parked();
    // Repeated paints catch a reveal that initially succeeds, then scrolls to the file bottom.
    for _ in 0..16 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.update(|window, cx| window.simulate_next_frame(cx));
        cx.run_until_parked();
    }
    cx.update(|_, cx| {
        let app = view.read(cx);
        let state = app.editor.read(cx);
        assert!(
            app.tabs[app.active_tab_index().unwrap()].definition_highlight_generation > 0,
            "navigation must reach the host callback"
        );
        assert_eq!(
            state.cursor_position(),
            expected,
            "jump must retain the definition caret"
        );
        let bounds = state
            .range_to_bounds(&target)
            .expect("definition must remain rendered");
        assert!(
            state.input_bounds().contains(&bounds.center()),
            "definition must remain visible: {bounds:?}"
        );
        // A definition near the beginning of a file clamps at zero instead of scrolling past it.
        if !declaration {
            assert!(
                (bounds.center().y - state.input_bounds().center().y).abs() < px(2.),
                "definition must be centered: {bounds:?}, viewport {:?}, scroll {:?}",
                state.input_bounds(),
                state.scroll_offset()
            );
        } else {
            assert_eq!(state.scroll_offset().y, px(0.));
        }
    });
}
