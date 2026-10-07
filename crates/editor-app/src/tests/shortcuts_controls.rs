//! Verifies edited bindings against real Base inputs, document sessions and native windows.

use super::shortcuts::{click, draw, second_editor_window, with_editor};
use crate::{EditorApp, ui::controls::Input};
use gpui_base::input::{InputEvent, InputState};
use gpui_kit::{
    AppContext as _, ClipboardItem, Context, Entity, Focusable, IntoElement, ParentElement, Render,
    Styled, Subscription, TestAppContext, VisualTestContext, Window, component::Root, div, gpui,
    px, size,
};
use std::{cell::RefCell, rc::Rc, time::Duration};

type EnterEvents = Rc<RefCell<Vec<(bool, bool)>>>;

/// Observe the public Input event without installing an alternative action or key handler.
struct NativeInputWindow {
    input: Entity<InputState>,
    _events: Subscription,
}

impl Render for NativeInputWindow {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().p_4().child(Input::new(&self.input))
    }
}

/// A second production Base Root receives the same runtime plugin as native settings dialogs.
fn native_input_window(
    visual: &mut VisualTestContext,
) -> (Entity<InputState>, VisualTestContext, EnterEvents) {
    let events = Rc::new(RefCell::new(Vec::new()));
    let recorded = events.clone();
    let mut input = None;
    let handle = visual.update(|_, cx| {
        cx.open_window(Default::default(), |window, cx| {
            let state = cx.new(|cx| InputState::new(window, cx));
            input = Some(state.clone());
            let view = cx.new(|cx| {
                let subscription = cx.subscribe(&state, move |_, _, event, _| {
                    if let InputEvent::PressEnter { secondary, shift } = event {
                        recorded.borrow_mut().push((*secondary, *shift));
                    }
                });
                NativeInputWindow {
                    input: state,
                    _events: subscription,
                }
            });
            cx.new(|cx| Root::new(view, window, cx))
        })
        .unwrap()
    });
    let input = input.unwrap();
    let mut native = VisualTestContext::from_window(handle.into(), &visual.cx);
    native.simulate_resize(size(px(600.), px(320.)));
    native.update(|window, cx| input.read(cx).focus_handle(cx).focus(window, cx));
    draw(&mut native);
    (input, native, events)
}

/// Search ordinary text so the selected operation stays tied to the original input scope.
fn search(visual: &mut VisualTestContext, title: &str) {
    click(visual, "shortcuts-search");
    visual.simulate_keystrokes("ctrl-a backspace");
    visual.simulate_input(title);
    draw(visual);
}

/// Settle the real capture deadline; saving still requires the visible explicit button.
fn rebind(visual: &mut VisualTestContext, selector: &'static str, keys: &str) {
    click(visual, selector);
    visual.simulate_keystrokes(keys);
    visual.executor().advance_clock(Duration::from_secs(2));
    draw(visual);
    click(visual, "shortcuts-edit-save");
    assert!(
        visual.debug_bounds("shortcuts-edit-capture").is_none(),
        "{selector}: confirmation={}, error={}",
        visual.debug_bounds("shortcuts-edit-confirmation").is_some(),
        visual.debug_bounds("shortcuts-edit-error").is_some(),
    );
}

/// Public clipboard contents distinguish actual Input selection copying from Root UI copying.
fn clipboard(visual: &VisualTestContext) -> String {
    visual
        .read_from_clipboard()
        .and_then(|item| item.text())
        .unwrap_or_default()
}

/// Retiring Input Ctrl+C must not copy its selection via the still-present Root Copy binding.
fn copies_only_with_new_key(visual: &mut VisualTestContext, keys: &str, expected: &str) {
    visual.simulate_keystrokes("ctrl-a");
    visual.write_to_clipboard(ClipboardItem::new_string("clipboard sentinel".into()));
    visual.simulate_keystrokes("ctrl-c");
    assert_eq!(clipboard(visual), "clipboard sentinel");
    visual.simulate_keystrokes(keys);
    assert_eq!(clipboard(visual), expected);
}

/// Read the document's own editing state; no test-side text buffer or dispatch map is involved.
fn document_text(visual: &mut VisualTestContext, app: &Entity<EditorApp>) -> String {
    visual.update(|_, cx| app.read(cx).editor.read(cx).text().to_string())
}

/// Copy, paste, undo and word deletion retain their real handlers across inputs and workspaces.
#[gpui::test]
fn shortcuts_controls_copy_and_document_edits_across_native_roots(cx: &mut TestAppContext) {
    with_editor(cx, false, vec![], |visual, app, path| {
        visual.simulate_keystrokes("ctrl-a");
        visual.simulate_input("alpha beta");
        visual.simulate_keystrokes("ctrl-a ctrl-c");
        assert_eq!(clipboard(visual), "alpha beta");
        visual.simulate_keystrokes("end ctrl-v");
        assert_eq!(document_text(visual, &app), "alpha betaalpha beta");
        visual.simulate_keystrokes("ctrl-z");
        assert_eq!(document_text(visual, &app), "alpha beta");

        // Both additional windows exist before the edit, proving publication is immediate.
        let (input, mut native, _) = native_input_window(visual);
        native.simulate_input("native input");
        let directory = tempfile::tempdir().unwrap();
        let (_, mut other) = second_editor_window(visual, directory.path());
        let copy_title = rust_i18n::t!("shortcuts.operation.Copy").to_string();
        visual.simulate_keystrokes("ctrl-k");
        draw(visual);
        search(visual, &copy_title);
        // The initial Panel tab selects Input Copy, not the separate Global Root Copy row.
        rebind(visual, "shortcut-binding-input::Copy-0", "ctrl-alt-c");
        click(visual, "shortcuts-search");
        copies_only_with_new_key(visual, "ctrl-alt-c", &copy_title);
        visual.simulate_keystrokes("escape");
        draw(visual);
        copies_only_with_new_key(visual, "ctrl-alt-c", "alpha beta");
        copies_only_with_new_key(&mut native, "ctrl-alt-c", "native input");
        copies_only_with_new_key(&mut other, "ctrl-alt-c", "second workspace");

        // Base Input's standard paste and undo handlers remain available in the other window.
        native.write_to_clipboard(ClipboardItem::new_string("replacement".into()));
        native.simulate_keystrokes("ctrl-v");
        assert_eq!(
            native.update(|_, cx| input.read(cx).value().to_string()),
            "replacement"
        );
        native.simulate_keystrokes("ctrl-z");
        assert_eq!(
            native.update(|_, cx| input.read(cx).value().to_string()),
            "native input"
        );

        visual.simulate_keystrokes("ctrl-k");
        draw(visual);
        search(visual, &copy_title);
        visual.write_to_clipboard(ClipboardItem::new_string("draft sentinel".into()));
        rebind(
            visual,
            "shortcut-binding-input::Copy-0",
            "ctrl-alt-c ctrl-alt-v",
        );
        assert_eq!(clipboard(visual), "draft sentinel");
        click(visual, "shortcuts-search");
        visual.simulate_keystrokes("ctrl-a ctrl-alt-c");
        draw(visual);
        assert!(visual.debug_bounds("shortcuts-pending").is_some());
        assert_eq!(clipboard(visual), "draft sentinel");
        visual.simulate_keystrokes("ctrl-alt-v");
        assert_eq!(clipboard(visual), copy_title);

        // Query capture must consume even an already-saved two-step editing shortcut.
        click(visual, "shortcuts-capture");
        visual.write_to_clipboard(ClipboardItem::new_string("capture sentinel".into()));
        visual.simulate_keystrokes("ctrl-alt-c ctrl-alt-v");
        draw(visual);
        assert_eq!(clipboard(visual), "capture sentinel");
        assert!(visual.debug_bounds("shortcuts-pending").is_none());
        // A long query stays on one line inside a small fixed-height search viewport.
        visual.simulate_resize(size(px(500.), px(430.)));
        draw(visual);
        let field = visual.debug_bounds("shortcuts-search").unwrap();
        let caps = visual.debug_bounds("shortcuts-search-keycaps").unwrap();
        assert!(caps.top() >= field.top());
        assert!(caps.bottom() <= field.bottom());
        visual.simulate_resize(size(px(1100.), px(850.)));
        draw(visual);
        visual.simulate_keystrokes("escape escape");
        draw(visual);
        copies_only_with_new_key(visual, "ctrl-alt-c ctrl-alt-v", "alpha beta");
        copies_only_with_new_key(&mut native, "ctrl-alt-c ctrl-alt-v", "native input");
        copies_only_with_new_key(&mut other, "ctrl-alt-c ctrl-alt-v", "second workspace");

        visual.simulate_keystrokes("ctrl-k");
        draw(visual);
        search(
            visual,
            &rust_i18n::t!("shortcuts.operation.DeleteToPreviousWordStart"),
        );
        rebind(
            visual,
            "shortcut-binding-input::DeleteToPreviousWordStart-0",
            "ctrl-alt-backspace",
        );
        visual.simulate_keystrokes("escape end ctrl-backspace");
        assert_eq!(document_text(visual, &app), "alpha beta");
        visual.simulate_keystrokes("ctrl-alt-backspace");
        assert_eq!(document_text(visual, &app), "alpha ");
        visual.simulate_keystrokes("ctrl-z");
        assert_eq!(document_text(visual, &app), "alpha beta");
        visual.simulate_keystrokes("ctrl-alt-backspace ctrl-s");
        draw(visual);
        assert_eq!(std::fs::read_to_string(path).unwrap(), "alpha ");
    });
}

/// The application menu survives removing its opener; Enter variants keep their own payloads.
#[gpui::test]
fn shortcuts_controls_opener_and_enter_variants(cx: &mut TestAppContext) {
    with_editor(cx, false, vec![], |visual, _, _| {
        let (_, mut native, events) = native_input_window(visual);
        let title = rust_i18n::t!("shortcuts.operation.OpenShortcuts").to_string();
        visual.simulate_keystrokes("ctrl-k alt-right");
        draw(visual);
        search(visual, &title);
        rebind(
            visual,
            "shortcut-binding-shortcuts::OpenShortcuts-0",
            "ctrl-alt-k",
        );
        visual.simulate_keystrokes("escape ctrl-k");
        draw(visual);
        assert!(visual.debug_bounds("shortcuts-panel").is_none());
        visual.simulate_keystrokes("ctrl-alt-k alt-right");
        draw(visual);
        search(visual, &title);
        click(visual, "shortcut-binding-shortcuts::OpenShortcuts-0");
        click(visual, "shortcuts-edit-delete");
        visual.simulate_keystrokes("escape ctrl-alt-k ctrl-k");
        draw(visual);
        assert!(visual.debug_bounds("shortcuts-panel").is_none());

        click(visual, "shortcuts-menu-trigger");
        click(visual, "native-menu-item-shortcuts");
        visual.simulate_keystrokes("alt-right");
        draw(visual);
        search(visual, &title);
        click(visual, "shortcut-add-shortcuts::OpenShortcuts");
        click(visual, "shortcuts-edit-restore");
        visual.simulate_keystrokes("escape ctrl-k");
        draw(visual);
        assert!(visual.debug_bounds("shortcuts-panel").is_some());

        // Enter belongs to Input's Panel scope, independent of the previously selected tab.
        click(visual, "shortcuts-tab-0");
        // Exact key search isolates plain Enter from same-name Shift/secondary variants.
        click(visual, "shortcuts-capture");
        visual.simulate_keystrokes("enter");
        visual.executor().advance_clock(Duration::from_secs(2));
        draw(visual);
        rebind(visual, "shortcut-binding-input::Enter-0", "ctrl-alt-9");
        visual.simulate_keystrokes("escape");
        draw(visual);
        native.simulate_keystrokes("enter");
        assert!(
            events.borrow().is_empty(),
            "old plain Enter must be retired"
        );
        native.simulate_keystrokes("ctrl-alt-9 shift-enter ctrl-enter");
        assert_eq!(
            *events.borrow(),
            vec![(false, false), (false, true), (true, false)]
        );
    });
}
