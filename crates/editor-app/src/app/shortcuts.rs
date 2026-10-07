//! Owns shortcut lookup, its captured panel context, and modal keyboard isolation.

pub(crate) mod bootstrap;
mod capture;
mod catalog;
mod config;
mod conflicts;
mod editing;
mod engine;
pub(crate) mod menu;
mod resolver;
mod runtime;
mod view;

use crate::*;
use capture::Capture;
use catalog::{Operation, Scope};
use gpui_base::input::InputState;
use gpui_kit::{Action, Global, KeyContext, Keystroke, KeystrokeEvent};

actions!(shortcuts, [OpenShortcuts]);

/// Install defaults once even when an application context opens more than one workspace.
struct Initialized;
impl Global for Initialized {}

/// Register host bindings at the shared application entry used by the executable and GPUI tests.
pub(crate) fn init(cx: &mut App) {
    if cx.has_global::<Initialized>() {
        return;
    }
    cx.bind_keys([
        KeyBinding::new("ctrl-k", OpenShortcuts, Some("EditorShell")),
        KeyBinding::new(
            "ctrl-s",
            SaveDocument,
            Some("EditorShell && !PluginSurface"),
        ),
        KeyBinding::new(
            "ctrl-shift-r",
            RefreshWorkspace,
            Some("EditorShell && !PluginSurface"),
        ),
        KeyBinding::new("ctrl-alt-t", ToggleTheme, Some("EditorShell")),
        KeyBinding::new(
            "f12",
            NavigateToDefinition,
            Some("EditorShell && !PluginSurface"),
        ),
        KeyBinding::new(
            "ctrl-i",
            ShowDefinitionDetails,
            Some("EditorShell && !PluginSurface"),
        ),
        KeyBinding::new("f8", NextSyntaxError, Some("EditorShell && !PluginSurface")),
        KeyBinding::new(
            "shift-f8",
            PreviousSyntaxError,
            Some("EditorShell && !PluginSurface"),
        ),
    ]);
    runtime::register(cx);
    cx.set_global(Initialized);
}

/// Associate an editor with its Base Root without requiring a second command dispatcher.
pub(crate) fn attach_window(window: &Window, owner: WeakEntity<EditorApp>, cx: &mut App) {
    // The executable/controlled fixture selects the profile before creating windows.
    // Reconcile late controls here without reading user files in unrelated application tests.
    if cx.has_global::<engine::BindingEngine>() {
        let _ = bootstrap::ensure(cx);
    }
    runtime::attach(window.window_handle(), owner, cx);
}

/// Ordinary modal input retains native text and focus actions while background commands stay out.
fn modal_action(name: &str) -> bool {
    name.starts_with("input::")
        || name.starts_with("ui::")
        || name.starts_with("dialog::")
        || matches!(name, "root::Tab" | "root::TabPrev")
}

/// Search state never replaces document text or undo in the existing editor session.
pub(crate) struct ShortcutPanel {
    owner: WeakEntity<EditorApp>,
    return_focus: FocusHandle,
    focus: FocusHandle,
    tabs_focus: FocusHandle,
    search: Entity<InputState>,
    operations: Vec<Operation>,
    /// Captured before moving focus, never recalculated from the modal's search field.
    contexts: Vec<KeyContext>,
    tab: usize,
    key_search: bool,
    capture: Capture,
    /// A staged binding remains separate from the shared, persisted effective keymap.
    draft: Option<editing::Draft>,
    confirm: Option<editing::Confirmation>,
    edit_error: Option<String>,
    scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

/// A pointer gesture retains the same pre-opening context as the keyboard entry.
pub(crate) struct ShortcutOrigin {
    focus: FocusHandle,
    contexts: Vec<KeyContext>,
    available: Vec<Box<dyn Action>>,
}

impl ShortcutOrigin {
    /// Read the current dispatch tree without changing focus or executing commands.
    fn capture(app: &EditorApp, window: &Window, cx: &App) -> Self {
        Self {
            focus: window
                .focused(cx)
                .unwrap_or_else(|| app.editor.focus_handle(cx)),
            contexts: window.context_stack(),
            available: window.available_actions(cx),
        }
    }
}

impl EditorApp {
    /// Capture original focus and available actions before the overlay takes focus.
    pub(crate) fn open_shortcuts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let origin = ShortcutOrigin::capture(self, window, cx);
        self.open_shortcuts_from(origin, window, cx);
    }

    /// Both entry paths use captured context and retain the same parameterized actions.
    fn open_shortcuts_from(
        &mut self,
        origin: ShortcutOrigin,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.shortcut_panel.is_some() {
            return;
        }
        self.shortcut_menu.clear();
        runtime::cancel(window.window_handle(), cx);
        let load_error = bootstrap::ensure(cx).err();
        runtime::sync_window_plugins(self, window.window_handle(), cx);
        let return_focus = origin.focus;
        let contexts = origin.contexts;
        let bindings: Vec<_> = if cx.has_global::<engine::BindingEngine>() {
            cx.global::<engine::BindingEngine>().defaults().to_vec()
        } else {
            cx.key_bindings().borrow().bindings().cloned().collect()
        };
        let mut available = origin.available;
        // Parameterized actions can have no default constructor while their handler is available.
        for binding in &bindings {
            if window.is_action_available_in(binding.action(), &return_focus)
                && !available
                    .iter()
                    .any(|action| action.name() == binding.action().name())
            {
                available.push(binding.action().boxed_clone());
            }
        }
        let entries = self.extensions.read(cx).entries.clone();
        let operations = catalog::all_operations(&bindings, &entries, cx);
        let mut operations = catalog::visible(&operations, &available, &contexts);
        if cx.has_global::<engine::BindingEngine>() {
            let engine = cx.global::<engine::BindingEngine>();
            operations.retain(|operation| engine.operation(&operation.id).is_some());
            for operation in &mut operations {
                operation.defaults = engine.effective(&operation.id);
            }
        }
        let owner = cx.entity().downgrade();
        let panel =
            cx.new(|cx| ShortcutPanel::new(owner, return_focus, contexts, operations, window, cx));
        panel.update(cx, |panel, cx| {
            panel.edit_error =
                load_error.map(|detail| t!("shortcuts.edit.storage", detail = detail).to_string());
            panel
                .search
                .update(cx, |search, cx| search.focus(window, cx))
        });
        self.shortcut_panel = Some(panel);
        cx.notify();
    }

    /// Restore the original live focus handle after removing the controlled overlay.
    fn close_shortcuts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = self.shortcut_panel.take() {
            let return_focus = panel.read(cx).return_focus.clone();
            return_focus.focus(window, cx);
        }
        cx.notify();
    }

    /// Keep a discoverable pointer entry when the opening binding is removed or changed.
    pub(crate) fn render_shortcuts_trigger(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .capture_any_mouse_down(cx.listener(|app, event: &MouseDownEvent, window, cx| {
                if event.button == MouseButton::Left {
                    app.shortcut_pointer_origin = Some(ShortcutOrigin::capture(app, window, cx));
                }
            }))
            .child(
                Button::new("shortcuts-trigger")
                    .debug_selector(|| "shortcuts-trigger".into())
                    .label(t!("shortcuts.title").to_string())
                    .small()
                    .ghost()
                    .on_click(cx.listener(|app, _, window, cx| {
                        let origin = app
                            .shortcut_pointer_origin
                            .take()
                            .unwrap_or_else(|| ShortcutOrigin::capture(app, window, cx));
                        app.open_shortcuts_from(origin, window, cx);
                    })),
            )
    }
}

impl ShortcutPanel {
    /// Connect Base input events and GPUI pre-dispatch interception within this one window.
    fn new(
        owner: WeakEntity<EditorApp>,
        return_focus: FocusHandle,
        contexts: Vec<KeyContext>,
        operations: Vec<Operation>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx
            .new(|cx| InputState::new(window, cx).placeholder(t!("shortcuts.search").to_string()));
        let changed = cx.subscribe(&search, |panel, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                panel.scroll.set_offset(Default::default());
                cx.notify();
            }
        });
        let weak = cx.entity().downgrade();
        let handle = window.window_handle();
        let interceptor = cx.intercept_keystrokes(move |event, window, cx| {
            if window.window_handle() == handle {
                let _ = weak.update(cx, |panel, cx| panel.on_keystroke(event, window, cx));
            }
        });
        let tab = usize::from(
            !operations
                .iter()
                .any(|operation| operation.scope == Scope::Panel),
        );
        Self {
            owner,
            return_focus,
            contexts,
            operations,
            search,
            tab,
            focus: cx.focus_handle(),
            tabs_focus: cx.focus_handle(),
            key_search: false,
            capture: Capture::default(),
            draft: None,
            confirm: None,
            edit_error: None,
            scroll: ScrollHandle::new(),
            _subscriptions: vec![changed, interceptor],
        }
    }

    /// Defer removal to avoid reading an entity during its own mutable input callback.
    fn request_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let owner = self.owner.clone();
        window.defer(cx, move |window, cx| {
            let _ = owner.update(cx, |app, cx| app.close_shortcuts(window, cx));
        });
    }

    /// Switch scope without clearing the current text or captured-key query.
    fn select_tab(&mut self, tab: usize, cx: &mut Context<Self>) {
        self.tab = tab.min(1);
        self.scroll.set_offset(Default::default());
        cx.notify();
    }

    /// Toggle key search and route focus away from background editing handlers.
    fn toggle_capture(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.key_search = !self.key_search;
        self.capture.clear();
        if self.key_search {
            self.focus.focus(window, cx);
        } else {
            self.search
                .update(cx, |search, cx| search.focus(window, cx));
        }
        cx.notify();
    }

    /// Capture precedes action dispatch; ordinary search permits only modal/input actions.
    fn on_keystroke(
        &mut self,
        event: &KeystrokeEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = &event.keystroke;
        if self.edit_keystroke(key, window, cx) {
            cx.stop_propagation();
            return;
        }
        if key.key == "escape" && key.modifiers == Modifiers::default() {
            cx.stop_propagation();
            if self.key_search {
                self.key_search = false;
                self.capture.clear();
                self.search
                    .update(cx, |search, cx| search.focus(window, cx));
                cx.notify();
            } else {
                self.request_close(window, cx);
            }
            return;
        }
        // While a draft is open, its original key query stays visible but does not record keys.
        // Save/cancel buttons must retain Base activation after the capture deadline.
        if self.key_search && self.draft.is_none() {
            cx.stop_propagation();
            self.record(key, cx);
            return;
        }
        if key.modifiers.alt
            && !key.modifiers.control
            && !key.modifiers.shift
            && !key.modifiers.platform
            && matches!(key.key.as_str(), "left" | "right")
        {
            cx.stop_propagation();
            self.request_edit_intent(
                editing::Intent::Tab(usize::from(key.key == "right")),
                window,
                cx,
            );
            return;
        }
        // The overlay blocks host/plugin bindings while text-editing keys stay with Base Input.
        let bound = cx
            .key_bindings()
            .borrow()
            .bindings()
            .filter(|binding| {
                // These keymap markers block ancestor actions; they are not background commands.
                // Preserve the focused Base button's own Enter activation behind NoAction.
                !gpui_kit::is_no_action(binding.action())
                    && !gpui_kit::is_unbind(binding.action())
                    && binding.keystrokes().len() == 1
                    && binding.keystrokes()[0].unparse() == key.unparse()
                    && binding
                        .predicate()
                        .is_none_or(|predicate| predicate.depth_of(&event.context_stack).is_some())
            })
            .map(|binding| binding.action().name().to_owned())
            .collect::<Vec<_>>();
        if !bound.is_empty() && !bound.iter().any(|name| modal_action(name)) {
            cx.stop_propagation();
        }
    }

    /// Show the first stroke immediately; the shared executor owns its two-second deadline.
    fn record(&mut self, key: &Keystroke, cx: &mut Context<Self>) {
        if !self.capture.record(key, cx.background_executor().now()) {
            return;
        }
        self.scroll.set_offset(Default::default());
        let generation = self.capture.generation;
        if self.capture.waiting {
            cx.spawn(async move |panel, cx| {
                cx.background_executor().timer(Duration::from_secs(2)).await;
                let _ = panel.update(cx, |panel, cx| {
                    if panel.capture.generation == generation {
                        panel.capture.waiting = false;
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        cx.notify();
    }

    /// Update only displayed bindings after the engine has persisted a successful mutation.
    fn sync_effective(&mut self, cx: &mut Context<Self>) {
        if let Some(engine) = cx.try_global::<engine::BindingEngine>() {
            for operation in &mut self.operations {
                operation.defaults = engine.effective(&operation.id);
            }
        }
        cx.notify();
    }
}
