//! Incarnation-bound adapter for OS text callbacks retained beyond a legacy canvas frame.
use super::*;
use gpui_kit::{ElementInputHandler, InputHandler, UTF16Selection};
use std::ops::Range;

/// Keeps the epoch at registration even when the panel entity survives a hot update.
pub(super) struct Handler {
    panel: Entity<ExtensionPanel>,
    epoch: u64,
    inner: ElementInputHandler<ExtensionPanel>,
}
impl Handler {
    /// Bind native text input to the rendered instance, rather than consulting the new epoch later.
    pub(super) fn new(bounds: Bounds<Pixels>, panel: Entity<ExtensionPanel>, epoch: u64) -> Self {
        Self {
            inner: ElementInputHandler::new(bounds, panel.clone()),
            panel,
            epoch,
        }
    }
    /// Old handlers remain invalid even after the replacement panel regains keyboard focus.
    fn current(&self, cx: &App) -> bool {
        self.panel.read(cx).instance_epoch == self.epoch
    }
}
impl InputHandler for Handler {
    fn selected_text_range(
        &mut self,
        ignore: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<UTF16Selection> {
        if !self.current(cx) {
            return None;
        }
        self.inner.selected_text_range(ignore, window, cx)
    }
    fn marked_text_range(&mut self, window: &mut Window, cx: &mut App) -> Option<Range<usize>> {
        if !self.current(cx) {
            return None;
        }
        self.inner.marked_text_range(window, cx)
    }
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<String> {
        if !self.current(cx) {
            return None;
        }
        self.inner.text_for_range(range, adjusted, window, cx)
    }
    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut App,
    ) {
        if !self.current(cx) {
            return;
        }
        self.inner.replace_text_in_range(range, text, window, cx)
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if !self.current(cx) {
            return;
        }
        self.inner
            .replace_and_mark_text_in_range(range, text, selected, window, cx)
    }
    fn unmark_text(&mut self, window: &mut Window, cx: &mut App) {
        if !self.current(cx) {
            return;
        }
        self.inner.unmark_text(window, cx)
    }
    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Bounds<Pixels>> {
        if !self.current(cx) {
            return None;
        }
        self.inner.bounds_for_range(range, window, cx)
    }
    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<usize> {
        if !self.current(cx) {
            return None;
        }
        self.inner.character_index_for_point(point, window, cx)
    }
    fn accepts_text_input(&mut self, window: &mut Window, cx: &mut App) -> bool {
        if !self.current(cx) {
            return false;
        }
        self.inner.accepts_text_input(window, cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{TestAppContext, gpui};

    /// OS callbacks retained across replacement cannot commit, inspect, or clear the new composition.
    #[gpui::test]
    fn old_ime_handler_stays_invalid_after_replacement_refocus(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            typography::init(cx);
            apply_theme(builtin_theme(false), cx);
        });
        let directory = tempfile::tempdir().unwrap();
        let workspace = Workspace::open(directory.path()).unwrap();
        let slot = Rc::new(RefCell::new(None));
        let capture = slot.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let app = cx.new(|cx| EditorApp::new(workspace, None, window, cx));
            *capture.borrow_mut() = Some(app.clone());
            Root::new(app, window, cx)
        });
        let app = slot.borrow_mut().take().unwrap();
        cx.update(|window, cx| {
            let panel = app.read(cx).extensions.clone();
            panel.update(cx, |panel, cx| {
                panel.active = Some("terminal".into());
                panel.worker.state.lock().unwrap().instance_epochs.insert("terminal".into(), 7);
                panel.poll(cx);
                panel.focus(window, cx);
            });
            let mut old = Handler::new(Bounds::default(), panel.clone(), 7);
            old.replace_and_mark_text_in_range(None, "old", None, window, cx);
            assert_eq!(panel.read(cx).composition, "old");
            panel.update(cx, |panel, cx| {
                panel.worker.state.lock().unwrap().instance_epochs.insert("terminal".into(), 8);
                panel.poll(cx);
                panel.focus(window, cx);
            });
            assert!(panel.read(cx).composition.is_empty());
            let mut current = Handler::new(Bounds::default(), panel.clone(), 8);
            current.replace_and_mark_text_in_range(None, "new", None, window, cx);
            old.replace_text_in_range(None, "late commit", window, cx);
            old.replace_and_mark_text_in_range(None, "late mark", None, window, cx);
            old.unmark_text(window, cx);
            assert!(old.marked_text_range(window, cx).is_none());
            assert_eq!(panel.read(cx).composition, "new");
            current.replace_text_in_range(None, "current commit", window, cx);
            let events: Vec<_> = panel.read(cx).worker.recorded.lock().unwrap().try_iter().collect();
            assert!(!events.iter().any(|event| matches!(event, Work::Event(_, _, PluginEvent::Text(text)) if text == "late commit")));
            assert!(events.iter().any(|event| matches!(event, Work::Event(_, 8, PluginEvent::Text(text)) if text == "current commit")));
        });
    }
}
