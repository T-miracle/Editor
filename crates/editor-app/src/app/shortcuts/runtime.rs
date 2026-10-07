//! Installs a single managed shortcut resolver in every native Base Root window.

use super::{
    catalog,
    engine::BindingEngine,
    resolver::{Resolution, Resolver},
};
use crate::*;
use gpui_base::{Root, RootPlugin};
use gpui_kit::{AnyWindowHandle, BorrowAppContext as _, Global, KeystrokeEvent};

/// Owners supply generic plugin availability; windows without an editor still support Input.
#[derive(Default)]
struct Registry {
    owners: HashMap<AnyWindowHandle, WeakEntity<EditorApp>>,
    runtimes: HashMap<AnyWindowHandle, WeakEntity<ShortcutRuntime>>,
    plugins: HashMap<AnyWindowHandle, Vec<catalog::Operation>>,
}
impl Global for Registry {}

/// Register before Root creation so settings and other native dialogs use this same mechanism.
pub(super) fn register(cx: &mut App) {
    cx.set_global(Registry::default());
    Root::register_plugin(cx, ShortcutRuntime::new);
}

/// Keep a weak owner; closing a workspace cannot retain its editor or plugin worker.
pub(super) fn attach(handle: AnyWindowHandle, owner: WeakEntity<EditorApp>, cx: &mut App) {
    cx.global_mut::<Registry>().owners.insert(handle, owner);
}

/// Publish ready commands for this workspace, using the ordinary public plugin command route.
/// The union retains bindings in a trusted window while another workspace is restricted.
pub(super) fn sync_window_plugins(owner: &EditorApp, handle: AnyWindowHandle, cx: &mut App) {
    let panel = owner.extensions.read(cx);
    let entries = panel.entries.clone();
    let mut operations = catalog::all_operations(&[], &entries, cx);
    operations.retain(|operation| match &operation.target {
        catalog::Target::Plugin { plugin, command } => {
            owner.session_state.workspace_trusted && panel.shortcut_available(plugin, command)
        }
        _ => false,
    });
    let registry = cx.global_mut::<Registry>();
    registry.owners.retain(|_, owner| owner.upgrade().is_some());
    registry
        .plugins
        .retain(|window, _| registry.owners.contains_key(window));
    registry.plugins.insert(handle, operations);
    let mut all = Vec::new();
    for operations in registry.plugins.values() {
        for operation in operations {
            if !all
                .iter()
                .any(|other: &catalog::Operation| other.id == operation.id)
            {
                all.push(operation.clone());
            }
        }
    }
    if cx.has_global::<BindingEngine>() {
        cx.update_global::<BindingEngine, _>(|engine, _| {
            engine.sync_plugin_operations(all);
        });
    }
}

/// A saved edit or lifecycle transition retires every window's old pending candidates.
pub(super) fn invalidate_all(cx: &mut App) {
    let runtimes = cx
        .global::<Registry>()
        .runtimes
        .values()
        .cloned()
        .collect::<Vec<_>>();
    for runtime in runtimes {
        let _ = runtime.update(cx, |runtime, cx| runtime.cancel(cx));
    }
    // A save can originate inside one of these panels. Refresh other open catalogs only
    // after that mutable entity callback has returned, using the same committed global state.
    cx.defer(|cx| {
        let owners = cx
            .global::<Registry>()
            .owners
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for owner in owners {
            if let Some(owner) = owner.upgrade()
                && let Some(panel) = owner.read(cx).shortcut_panel.clone()
            {
                panel.update(cx, |panel, cx| panel.sync_effective(cx));
            }
        }
    });
}

/// Modal entry cancels just this window's sequence before focus moves to recording/search.
pub(super) fn cancel(handle: AnyWindowHandle, cx: &mut App) {
    let runtime = cx.global::<Registry>().runtimes.get(&handle).cloned();
    if let Some(runtime) = runtime {
        let _ = runtime.update(cx, |runtime, cx| runtime.cancel(cx));
    }
}

/// The Root owns both the resolver's lifetime and its noninteractive next-step prompt.
struct ShortcutRuntime {
    resolver: Resolver,
    conflict: Vec<super::conflicts::Conflict>,
    blur: Option<Subscription>,
    _subscriptions: Vec<Subscription>,
}

impl ShortcutRuntime {
    /// Subscribe once for this window, rather than adding another raw plugin key loop.
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        if cx.has_global::<BindingEngine>() {
            let _ = super::bootstrap::ensure(cx);
        }
        let handle = window.window_handle();
        let weak = cx.weak_entity();
        cx.global_mut::<Registry>()
            .runtimes
            .insert(handle, weak.clone());
        let keys = cx.intercept_keystrokes(move |event, window, cx| {
            if window.window_handle() == handle {
                let _ = weak.update(cx, |runtime, cx| runtime.on_key(event, window, cx));
            }
        });
        let activation = cx.observe_window_activation(window, |runtime, window, cx| {
            if !window.is_window_active() {
                runtime.cancel(cx);
            }
        });
        Self {
            resolver: Resolver::default(),
            conflict: Vec::new(),
            blur: None,
            _subscriptions: vec![keys, activation],
        }
    }

    /// Resolve against real handlers, original predicates and live plugin admission checks.
    fn on_key(&mut self, event: &KeystrokeEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !cx.has_global::<BindingEngine>() {
            return;
        }
        let contexts = window.context_stack();
        let owner = cx
            .global::<Registry>()
            .owners
            .get(&window.window_handle())
            .cloned()
            .and_then(|owner| owner.upgrade());
        let panel = owner
            .as_ref()
            .and_then(|owner| owner.read(cx).shortcut_panel.clone());
        let modal = panel.is_some()
            || contexts
                .iter()
                .any(|context| context.contains("ShortcutPanel"));
        let recording = panel.as_ref().is_some_and(|panel| {
            let panel = panel.read(cx);
            panel.key_search || panel.draft.is_some() || panel.confirm.is_some()
        });
        if recording {
            self.cancel(cx);
            return;
        }
        // Copy the owner read into a separate update: no editor is mutably borrowed here.
        if let Some(owner) = &owner {
            owner.update(cx, |owner, cx| {
                sync_window_plugins(owner, window.window_handle(), cx)
            });
        }
        cx.update_global::<BindingEngine, _>(|engine, _| engine.observe_context(&contexts));
        let focus = window.focused(cx);
        let now = cx.background_executor().now();
        let resolution = self.resolver.resolve(
            cx.global::<BindingEngine>(),
            &event.keystroke,
            window.window_handle(),
            focus.clone(),
            &contexts,
            now,
            |target| match target {
                catalog::Target::Native { action, .. } => {
                    (!modal || super::modal_action(action.name()))
                        && focus.as_ref().is_some_and(|focus| {
                            window.is_action_available_in(action.as_ref(), focus)
                        })
                }
                catalog::Target::Plugin { plugin, command } => {
                    !modal
                        && owner.as_ref().is_some_and(|owner| {
                            let owner = owner.read(cx);
                            owner.session_state.workspace_trusted
                                && owner
                                    .extensions
                                    .read(cx)
                                    .shortcut_available(plugin, command)
                        })
                }
            },
        );
        self.conflict.clear();
        match resolution {
            Resolution::Pass => {
                if self.resolver.pending().is_none() {
                    self.blur = None;
                }
            }
            Resolution::Pending(hint) => {
                cx.stop_propagation();
                if let Some(focus) = focus {
                    self.blur =
                        Some(cx.on_blur(&focus, window, |runtime, _, cx| runtime.cancel(cx)));
                }
                let delay = hint.deadline.saturating_duration_since(now);
                cx.spawn_in(window, async move |runtime, cx| {
                    cx.background_executor().timer(delay).await;
                    let _ = runtime.update(cx, |runtime, cx| {
                        if runtime.resolver.expire(cx.background_executor().now()) {
                            runtime.blur = None;
                            cx.notify();
                        }
                    });
                })
                .detach();
            }
            Resolution::Dispatch { target, .. } => {
                cx.stop_propagation();
                self.blur = None;
                match target {
                    catalog::Target::Native { action, .. } => window.dispatch_action(action, cx),
                    catalog::Target::Plugin { plugin, command } => {
                        if let Some(owner) = owner {
                            let extension = owner.read(cx).extensions.clone();
                            extension.update(cx, |panel, cx| {
                                panel.invoke_shortcut(&plugin, &command, window, cx);
                            });
                        }
                    }
                }
            }
            Resolution::Conflict(conflicts) => {
                cx.stop_propagation();
                self.blur = None;
                self.conflict = conflicts;
            }
        }
        cx.notify();
    }

    /// A consumed prefix is cancelled, never replayed into an editor or another window.
    fn cancel(&mut self, cx: &mut Context<Self>) {
        let changed = self.resolver.cancel() || !self.conflict.is_empty();
        self.conflict.clear();
        self.blur = None;
        if changed {
            cx.notify();
        }
    }
}

impl RootPlugin for ShortcutRuntime {
    fn prepare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The first Root render has no dispatch tree yet. Inspect it only for a live sequence.
        if self.resolver.pending().is_none() {
            return;
        }
        if let Some(engine) = cx.try_global::<BindingEngine>() {
            // Root is already repainting. This hook must not enqueue another notification.
            if self.resolver.invalidate_if_changed(
                engine.revision(),
                window.window_handle(),
                window.focused(cx),
                &window.context_stack(),
                cx.background_executor().now(),
            ) {
                self.blur = None;
            }
        }
    }
}

impl Render for ShortcutRuntime {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(hint) = self.resolver.pending() {
            return crate::ui::controls::shortcut_pending_hint(
                t!("shortcuts.pending").to_string(),
                hint.next
                    .iter()
                    .map(|next| (next.title.clone(), super::capture::display(&next.sequence)))
                    .collect(),
                cx,
            );
        }
        if !self.conflict.is_empty() {
            return crate::ui::controls::shortcut_pending_hint(
                t!("shortcuts.edit.conflict").to_string(),
                self.conflict
                    .iter()
                    .map(|conflict| {
                        (
                            conflict.title.clone(),
                            super::capture::display(&conflict.binding),
                        )
                    })
                    .collect(),
                cx,
            );
        }
        div().into_any_element()
    }
}
