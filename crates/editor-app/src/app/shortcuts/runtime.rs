//! Installs a single managed shortcut resolver in every native Base Root window.

use super::{
    catalog,
    engine::BindingEngine,
    resolver::{Resolution, Resolver},
};
use crate::*;
use gpui_base::{Root, RootPlugin};
use gpui_kit::{AnyWindowHandle, BorrowAppContext as _, Global, KeystrokeEvent};
use std::collections::BTreeMap;

/// Only lifecycle facts change this identity; ordinary worker polling must not cancel chords.
#[derive(PartialEq, Eq)]
struct PluginFingerprint {
    trusted: bool,
    ready: bool,
    entries: Vec<(String, String, bool, Option<String>)>,
    startup: BTreeMap<String, String>,
    epochs: BTreeMap<(String, String), u64>,
}

/// Metadata and command epochs come from one worker publication, never mixed incarnations.
struct WindowPlugins {
    fingerprint: PluginFingerprint,
    operations: Vec<catalog::Operation>,
}

/// Owners supply generic plugin availability; windows without an editor still support Input.
#[derive(Default)]
struct Registry {
    owners: HashMap<AnyWindowHandle, WeakEntity<EditorApp>>,
    runtimes: HashMap<AnyWindowHandle, WeakEntity<ShortcutRuntime>>,
    plugins: HashMap<AnyWindowHandle, WindowPlugins>,
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

/// Poll callbacks have an owner but no Window; resolve its already registered native window.
pub(super) fn owner_window(owner: gpui_kit::EntityId, cx: &App) -> Option<AnyWindowHandle> {
    cx.global::<Registry>()
        .owners
        .iter()
        .find_map(|(window, candidate)| (candidate.entity_id() == owner).then_some(*window))
}

/// Window-local catalogs must not inherit another workspace's trusted plugin contributions.
pub(super) fn window_operations(handle: AnyWindowHandle, cx: &App) -> Vec<catalog::Operation> {
    cx.global::<Registry>()
        .plugins
        .get(&handle)
        .map(|state| state.operations.clone())
        .unwrap_or_default()
}

/// Publish ready commands for this workspace, using the ordinary public plugin command route.
/// The union retains bindings in a trusted window while another workspace is restricted.
pub(super) fn sync_window_plugins(owner: &EditorApp, handle: AnyWindowHandle, cx: &mut App) {
    let snapshot = owner.extensions.read(cx).shortcut_snapshot();
    let trusted = owner.session_state.workspace_trusted && snapshot.trusted;
    let mut entries = snapshot
        .entries
        .iter()
        .map(|entry| {
            (
                entry.manifest.id.clone(),
                entry.digest.clone(),
                entry.enabled,
                entry.error.clone(),
            )
        })
        .collect::<Vec<_>>();
    entries.sort();
    let fingerprint = PluginFingerprint {
        trusted,
        ready: snapshot.ready,
        entries,
        startup: snapshot.startup,
        epochs: snapshot.commands,
    };
    // A same-digest replacement still changes the command epoch. Unchanged polls return
    // before rebuilding catalogs, touching engine revisions or notifying any native surface.
    // Closed owners must be retired even when this window's own publication is unchanged.
    // Otherwise their inactive bindings could remain as phantom conflict candidates forever.
    let stale_owner = cx
        .global::<Registry>()
        .owners
        .values()
        .any(|owner| owner.upgrade().is_none());
    if !stale_owner
        && cx
            .global::<Registry>()
            .plugins
            .get(&handle)
            .is_some_and(|old| old.fingerprint == fingerprint)
    {
        return;
    }
    let mut operations = catalog::all_operations(&[], &snapshot.entries, cx);
    operations.retain(|operation| match &operation.target {
        catalog::Target::Plugin { plugin, command } => {
            trusted
                && fingerprint.ready
                && fingerprint
                    .epochs
                    .contains_key(&(plugin.clone(), command.clone()))
        }
        _ => false,
    });
    let registry = cx.global_mut::<Registry>();
    registry.owners.retain(|_, owner| owner.upgrade().is_some());
    registry
        .plugins
        .retain(|window, _| registry.owners.contains_key(window));
    registry.plugins.insert(
        handle,
        WindowPlugins {
            fingerprint,
            operations,
        },
    );
    let mut all = Vec::new();
    for state in registry.plugins.values() {
        for operation in &state.operations {
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
            // Operation IDs/defaults alone cannot detect replacing an identical package instance.
            engine.invalidate_pending();
        });
    }
    invalidate_all(cx);
}

/// A saved edit or lifecycle transition retires every window's old pending candidates.
pub(super) fn invalidate_all(cx: &mut App) {
    // Synchronization also runs inside a resolver callback and saves inside a panel callback.
    // Defer both kinds of entity update; engine revision already blocks stale dispatch now.
    cx.defer(|cx| {
        let runtimes = cx
            .global::<Registry>()
            .runtimes
            .iter()
            .map(|(window, runtime)| (*window, runtime.clone()))
            .collect::<Vec<_>>();
        for (handle, runtime) in runtimes {
            let _ = cx.update_window(handle, |_, window, cx| {
                let _ = runtime.update(cx, |runtime, cx| {
                    // A key callback can already start a fresh sequence under the new revision
                    // before this defer runs. Retire only stale pending state, not that new chord.
                    let revision = cx
                        .try_global::<BindingEngine>()
                        .map(BindingEngine::revision)
                        .unwrap_or(0);
                    let changed = runtime.resolver.invalidate_if_changed(
                        revision,
                        handle,
                        window.focused(cx),
                        &window.context_stack(),
                        cx.background_executor().now(),
                    ) || !runtime.conflict.is_empty();
                    runtime.conflict.clear();
                    if runtime.resolver.pending().is_none() {
                        runtime.blur = None;
                    }
                    if changed {
                        cx.notify();
                    }
                });
            });
        }
        let owners = cx
            .global::<Registry>()
            .owners
            .iter()
            .map(|(window, owner)| (*window, owner.clone()))
            .collect::<Vec<_>>();
        for (handle, owner) in owners {
            if let Some(owner) = owner.upgrade()
                && let Some(panel) = owner.read(cx).shortcut_panel.clone()
            {
                let operations = window_operations(handle, cx);
                let _ = cx.update_window(handle, |_, window, cx| {
                    panel.update(cx, |panel, cx| {
                        panel.sync_plugin_operations(operations, window, cx)
                    });
                });
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
        // Pin this event to the publication used to resolve it. A newer worker instance
        // must not receive an old sequence merely because its stable command ID is equal.
        let epochs = cx
            .global::<Registry>()
            .plugins
            .get(&window.window_handle())
            .map(|state| state.fingerprint.epochs.clone())
            .unwrap_or_default();
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
                                && epochs.get(&(plugin.clone(), command.clone())).is_some_and(
                                    |epoch| {
                                        owner.extensions.read(cx).shortcut_epoch(plugin, command)
                                            == Some(*epoch)
                                    },
                                )
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
                        if let Some(owner) = owner
                            && let Some(epoch) =
                                epochs.get(&(plugin.clone(), command.clone())).copied()
                        {
                            let extension = owner.read(cx).extensions.clone();
                            extension.update(cx, |panel, cx| {
                                panel
                                    .invoke_shortcut_at_epoch(&plugin, &command, epoch, window, cx);
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
