//! Exercise completion navigation and deletion through the real editor key bindings.

use super::*;
use editor_core::Workspace;
use gpui_base::input::{CompletionProvider, Rope};
use gpui_kit::{TestAppContext, VisualTestContext, component::Root, gpui};
use lsp_types::{CompletionContext, CompletionItem, CompletionResponse};
use std::cell::RefCell;

/// Open an isolated document so completion tests never depend on installed servers.
fn completion_editor(
    cx: &mut TestAppContext,
) -> (tempfile::TempDir, Entity<EditorApp>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        typography::init(cx);
        theme::apply_theme(theme::builtin_theme(false), cx);
        cx.set_reduce_motion(true);
    });
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("sample.txt");
    std::fs::write(&path, "").unwrap();
    let workspace = Workspace::open(directory.path()).unwrap();
    let slot = Rc::new(RefCell::new(None));
    let capture = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| EditorApp::new(workspace, Some(path), window, cx));
        *capture.borrow_mut() = Some(view.clone());
        Root::new(view, window, cx)
    });
    let view = slot.borrow_mut().take().unwrap();
    cx.simulate_resize(size(px(1000.), px(800.)));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    (directory, view, cx)
}

/// Moving past the viewport and back must keep the entire selected row visible.
#[gpui::test]
fn completion_keyboard_selection_stays_visible(cx: &mut TestAppContext) {
    let (_directory, view, cx) = completion_editor(cx);
    cx.update(|_, cx| {
        view.read(cx).editor.clone().update(cx, |state, cx| {
            state.present_completion_items(
                0,
                "",
                (0..24)
                    .map(|index| CompletionItem {
                        label: format!("candidate_{index:02}"),
                        ..Default::default()
                    })
                    .collect(),
                cx,
            );
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    for (key, selected, selector) in [
        ("down", 23, "editor-completion-row-23"),
        ("up", 0, "editor-completion-row-0"),
    ] {
        for _ in 0..23 {
            cx.simulate_keystrokes(key);
        }
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert_eq!(
            cx.update(|_, cx| view.read(cx).completion_popup.selection.get().1),
            selected
        );
        let panel = cx.debug_bounds("editor-completion-card").unwrap();
        let row = cx.debug_bounds(selector).unwrap();
        assert!(
            row.top() >= panel.top() && row.bottom() <= panel.bottom(),
            "selected completion {selected} must stay inside the panel: {row:?} / {panel:?}"
        );
    }
}

/// Record requests and widen the candidate set when the user shortens the prefix.
struct DeletionCompletions {
    requests: Rc<RefCell<Vec<String>>>,
    /// Tests can hold refresh responses to check cancellation and out-of-order delivery.
    pending: Option<Rc<RefCell<Vec<futures::channel::oneshot::Sender<CompletionResponse>>>>>,
}

impl CompletionProvider for DeletionCompletions {
    /// Ready responses make the key-to-refresh assertion deterministic.
    fn completions(
        &self,
        text: &Rope,
        _offset: usize,
        _trigger: CompletionContext,
        _window: &mut Window,
        cx: &mut App,
    ) -> gpui_kit::Task<anyhow::Result<CompletionResponse>> {
        let source = text.to_string();
        self.requests.borrow_mut().push(source.clone());
        if source != "pri"
            && let Some(pending) = &self.pending
        {
            let (sender, receiver) = futures::channel::oneshot::channel();
            pending.borrow_mut().push(sender);
            return cx.spawn(async move |_| Ok(receiver.await?));
        }
        let labels = if source == "pri" {
            vec!["print"]
        } else {
            vec!["print", "probe"]
        };
        gpui_kit::Task::ready(Ok(CompletionResponse::Array(
            labels
                .into_iter()
                .map(|label| CompletionItem {
                    label: label.into(),
                    ..Default::default()
                })
                .collect(),
        )))
    }

    /// Model the production provider's identifier trigger policy.
    fn is_completion_trigger(&self, _offset: usize, new_text: &str, _cx: &mut App) -> bool {
        !new_text.is_empty() && new_text.chars().all(char::is_alphanumeric)
    }
}

/// Backspace must retain the panel and fetch candidates for the shortened input.
#[gpui::test]
fn completion_backspace_refreshes_visible_candidates(cx: &mut TestAppContext) {
    let (_directory, view, cx) = completion_editor(cx);
    let requests = Rc::new(RefCell::new(Vec::new()));
    cx.update(|_, cx| {
        view.read(cx).editor.clone().update(cx, |state, _| {
            state.lsp_mut().completion_provider = Some(Rc::new(DeletionCompletions {
                requests: requests.clone(),
                pending: None,
            }));
        });
    });
    cx.simulate_input("pri");
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.debug_bounds("editor-completion-card").is_some());
    cx.simulate_keystrokes("backspace");
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|_, cx| {
        let state = view.read(cx).editor.read(cx);
        assert_eq!(state.text().to_string(), "pr");
        assert!(
            state.completion_menu_state().open,
            "backspace must retain the panel"
        );
        assert_eq!(state.completion_menu_state().items.len(), 2);
    });
    assert_eq!(requests.borrow().last().map(String::as_str), Some("pr"));
}

/// The forward Delete binding must refresh using the same native editing path.
#[gpui::test]
fn completion_forward_delete_refreshes_candidates(cx: &mut TestAppContext) {
    let (_directory, view, cx) = completion_editor(cx);
    let requests = Rc::new(RefCell::new(Vec::new()));
    cx.update(|_, cx| {
        view.read(cx).editor.clone().update(cx, |state, _| {
            state.lsp_mut().completion_provider = Some(Rc::new(DeletionCompletions {
                requests: requests.clone(),
                pending: None,
            }));
        });
    });
    cx.simulate_input("pri");
    cx.run_until_parked();
    cx.update(|window, cx| {
        view.read(cx).editor.clone().update(cx, |state, cx| {
            let items = state.completion_menu_state().items.clone();
            state.set_cursor_position(lsp_types::Position::new(0, 2), window, cx);
            state.present_completion_items(0, "pr", items, cx);
        });
        window.draw(cx).clear(cx);
    });
    cx.simulate_keystrokes("delete");
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|_, cx| {
        let state = view.read(cx).editor.read(cx);
        assert_eq!(state.text().to_string(), "pr");
        assert!(state.completion_menu_state().open);
        assert_eq!(state.completion_menu_state().items.len(), 2);
    });
    assert_eq!(requests.borrow().last().map(String::as_str), Some("pr"));
}

/// Pending edits cannot be inserted, and an older response cannot replace a newer list.
#[gpui::test]
fn completion_deletion_refresh_keeps_panel_and_discards_stale_responses(cx: &mut TestAppContext) {
    let (_directory, view, cx) = completion_editor(cx);
    let requests = Rc::new(RefCell::new(Vec::new()));
    let pending = Rc::new(RefCell::new(Vec::new()));
    cx.update(|_, cx| {
        view.read(cx).editor.clone().update(cx, |state, _| {
            state.lsp_mut().completion_provider = Some(Rc::new(DeletionCompletions {
                requests: requests.clone(),
                pending: Some(pending.clone()),
            }));
        });
    });
    cx.simulate_input("pri");
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    // simulate_input types one character at a time; earlier typing requests were cancelled.
    pending.borrow_mut().clear();
    cx.simulate_keystrokes("backspace");
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let row = cx.debug_bounds("editor-completion-row-0").unwrap();
    assert!(cx.debug_bounds("editor-completion-card").is_some());
    cx.simulate_keystrokes("enter");
    cx.simulate_click(row.center(), Default::default());
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(view.read(cx).editor.read(cx).text().to_string(), "pr");
    });

    cx.simulate_keystrokes("backspace");
    cx.run_until_parked();
    assert_eq!(pending.borrow().len(), 2);
    pending
        .borrow_mut()
        .pop()
        .unwrap()
        .send(CompletionResponse::Array(vec![CompletionItem {
            label: "push".into(),
            ..Default::default()
        }]))
        .unwrap();
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    pending
        .borrow_mut()
        .pop()
        .unwrap()
        .send(CompletionResponse::Array(vec![CompletionItem {
            label: "outdated".into(),
            ..Default::default()
        }]))
        .unwrap();
    cx.run_until_parked();
    cx.update(|_, cx| {
        let state = view.read(cx).editor.read(cx);
        assert_eq!(state.text().to_string(), "p");
        assert_eq!(state.completion_menu_state().items[0].label, "push");
    });

    // Even deleting the entire prefix must request candidates; Escape cancels that request.
    cx.simulate_keystrokes("backspace");
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.debug_bounds("editor-completion-card").is_some());
    assert_eq!(requests.borrow().last().map(String::as_str), Some(""));
    cx.simulate_keystrokes("escape");
    pending
        .borrow_mut()
        .pop()
        .unwrap()
        .send(CompletionResponse::Array(vec![CompletionItem {
            label: "cancelled".into(),
            ..Default::default()
        }]))
        .unwrap();
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.debug_bounds("editor-completion-card").is_none());
}

/// Supply unfiltered candidates through the production matcher and replacement range logic.
struct FuzzyCompletions;

impl CompletionProvider for FuzzyCompletions {
    /// Match against the exact byte cursor, including any non-ASCII text before it.
    fn completions(
        &self,
        text: &Rope,
        offset: usize,
        _trigger: CompletionContext,
        _window: &mut Window,
        _cx: &mut App,
    ) -> gpui_kit::Task<anyhow::Result<CompletionResponse>> {
        let prefix = crate::language::completion::identifier_prefix(&text.to_string(), offset);
        gpui_kit::Task::ready(Ok(crate::language::completion::rank_completions(
            CompletionResponse::Array(vec![CompletionItem {
                label: "println".into(),
                ..Default::default()
            }]),
            &prefix,
            text.offset_to_position(offset - prefix.len()),
            text.offset_to_position(offset),
        )))
    }

    /// Let this fixture exercise a complete typed member expression in one input event.
    fn is_completion_trigger(&self, _offset: usize, new_text: &str, _cx: &mut App) -> bool {
        !new_text.is_empty()
    }
}

/// Accepting a fuzzy candidate replaces the abbreviation without damaging preceding UTF-8 text.
#[gpui::test]
fn completion_fuzzy_candidate_replaces_abbreviation(cx: &mut TestAppContext) {
    let (_directory, view, cx) = completion_editor(cx);
    cx.update(|_, cx| {
        view.read(cx).editor.clone().update(cx, |state, _| {
            state.lsp_mut().completion_provider = Some(Rc::new(FuzzyCompletions));
        });
    });
    cx.simulate_input("变量.pn");
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.debug_bounds("editor-completion-card").is_some());
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(
            view.read(cx).editor.read(cx).text().to_string(),
            "变量.println"
        );
    });
}
