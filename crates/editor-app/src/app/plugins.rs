//! Shares runtime-log reminders with a fixed summary snapshot beside the status bar.

use crate::extensions::{level_label, log_time, severity_icon};
use crate::ui::controls::{Spinner, StatusIcon};
use crate::*;
use gpui_base::PopoverState;
use plugin_runtime::logs::{LogLevel, LogRecord};
use std::collections::BTreeMap;
mod status_popup;

/// One status-bar entry presents the highest pending anomaly, then normal loading.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PluginPopupKind {
    Loading,
    Warning,
    Error,
}

/// Captured records never change while open; the base state owns focus and dismissal resources.
pub(crate) struct PluginPopupSnapshot {
    summaries: Vec<PluginSummary>,
    state: Entity<PopoverState>,
    scroll: ScrollHandle,
    /// A fresh identity rejects paint callbacks from a closed or replaced popup.
    token: Rc<()>,
    /// The original trigger can disappear and return with the same kind but a different Base focus identity.
    trigger: PluginPopupKind,
    trigger_removed: bool,
    /// Focus observers live only as long as their currently routable rows and this popup.
    row_focus: BTreeMap<String, PluginPopupRowFocus>,
}

/// The current layout index changes with loading tasks; the handle remains stable across repaints.
struct PluginPopupRowFocus {
    handle: FocusHandle,
    index: usize,
    _subscription: Subscription,
}

/// Store the installed display name with the event so uninstall cannot rewrite an open summary.
struct PluginSummary {
    name: String,
    record: LogRecord,
}

/// Live loading is grouped by ownership without interpreting localized messages or plugin IDs.
struct PluginLoadingSummary {
    plugin: Option<String>,
    name: String,
    details: Vec<String>,
}

impl EditorApp {
    /// Persist authority outside the repository and immediately withdraw all workspace presentation.
    pub(crate) fn set_workspace_trusted(&mut self, trusted: bool, cx: &mut Context<Self>) {
        self.session_state.workspace_trusted = trusted;
        self.persist_session();
        self.extensions
            .update(cx, |panel, cx| panel.set_workspace_trusted(trusted, cx));
        // The editor window (not the settings dialog) owns dock and editor synchronization.
        self.pending_contribution_sync = true;
        self.refresh_dialog(cx);
        cx.notify();
    }

    /// Reconcile every installed declaration through the same generation-checked provider registry.
    pub(crate) fn sync_runtime_contributions(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        apply_theme(&theme::active_theme(self.dark_theme), cx);
        self.sync_dynamic_languages(cx);
        self.reset_syntax_diagnostics(cx);
        cx.notify();
    }

    /// Counts current reminders, rather than repeatedly advertising already-confirmed failures.
    pub(crate) fn plugin_count(&self, kind: PluginPopupKind, cx: &App) -> usize {
        match kind {
            PluginPopupKind::Loading => self.plugin_loading_groups(cx).len(),
            PluginPopupKind::Warning | PluginPopupKind::Error => self
                .extensions
                .read(cx)
                .runtime_logs()
                .pending_reminders()
                .iter()
                .filter(|(_, level)| {
                    *level
                        == if kind == PluginPopupKind::Error {
                            LogLevel::Error
                        } else {
                            LogLevel::Warning
                        }
                })
                .count(),
        }
    }

    /// A single entry guarantees anomalies are never hidden by simultaneous normal loading.
    pub(crate) fn plugin_indicator(&self, cx: &App) -> Option<PluginPopupKind> {
        let severity = self
            .extensions
            .read(cx)
            .runtime_logs()
            .pending_reminders()
            .into_iter()
            .map(|(_, level)| level)
            .max();
        match severity {
            Some(LogLevel::Error) => Some(PluginPopupKind::Error),
            Some(LogLevel::Warning) => Some(PluginPopupKind::Warning),
            _ if self.plugin_count(PluginPopupKind::Loading, cx) > 0 => {
                Some(PluginPopupKind::Loading)
            }
            _ => None,
        }
    }

    /// Remember actual rendered absence, even when a later arrival recreates an entry of the same severity.
    pub(crate) fn track_plugin_indicator_frame(&mut self, indicator: Option<PluginPopupKind>) {
        if let Some(snapshot) = &mut self.plugin_popup_snapshot {
            snapshot.trigger_removed |= indicator != Some(snapshot.trigger);
        }
    }

    /// Capture records and their confirmation boundary atomically so concurrent arrivals stay outside this viewing round.
    fn toggle_plugin_popup(
        &mut self,
        kind: PluginPopupKind,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        if self.plugin_popup.is_some() {
            self.close_plugin_popup(window, cx);
            return;
        }
        let panel = self.extensions.read(cx);
        let logs = panel.runtime_logs();
        let (checkpoint, records) = logs.reminder_snapshot();
        let summaries: Vec<_> = records
            .into_iter()
            .map(|record| {
                let name = panel
                    .entries
                    .iter()
                    .find(|entry| entry.manifest.id == record.plugin)
                    .map_or_else(
                        || record.plugin.clone(),
                        |entry| entry.manifest.name.clone(),
                    );
                PluginSummary { name, record }
            })
            .collect();
        let token = Rc::new(());
        let state = cx.new(|cx| PopoverState::new(false, cx));
        let parent = cx.entity().downgrade();
        let captured = token.clone();
        state.update(cx, |state, _| {
            state.set_on_open_change(Some(Rc::new(move |open, _, cx| {
                if !*open {
                    let parent = parent.clone();
                    let captured = captured.clone();
                    cx.defer(move |cx| {
                        let _ = parent.update(cx, |app, cx| {
                            if app
                                .plugin_popup_snapshot
                                .as_ref()
                                .is_some_and(|snapshot| Rc::ptr_eq(&snapshot.token, &captured))
                            {
                                app.plugin_popup = None;
                                app.plugin_popup_snapshot = None;
                                cx.notify();
                            }
                        });
                    });
                }
            })));
        });
        // Existing completion callbacks may close a pure loading list. An anomaly snapshot remains reviewable.
        let trigger = kind;
        let kind = if kind == PluginPopupKind::Loading && !summaries.is_empty() {
            if summaries
                .iter()
                .any(|summary| summary.record.level == LogLevel::Error)
            {
                PluginPopupKind::Error
            } else {
                PluginPopupKind::Warning
            }
        } else {
            kind
        };
        self.plugin_popup = Some((kind, event.position()));
        self.plugin_popup_snapshot = Some(PluginPopupSnapshot {
            summaries,
            state: state.clone(),
            scroll: ScrollHandle::new(),
            token,
            trigger,
            trigger_removed: false,
            row_focus: BTreeMap::new(),
        });
        logs.confirm_reminders(&checkpoint);
        state.update(cx, |state, cx| state.show(window, cx));
        cx.notify();
    }

    /// Base popover dismissal restores focus and releases its deferred-popup registration; logs stay intact.
    pub(crate) fn close_plugin_popup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.plugin_popup = None;
        if let Some(snapshot) = self.plugin_popup_snapshot.take() {
            let focused = snapshot
                .state
                .read(cx)
                .focus_handle(cx)
                .contains_focused(window, cx);
            snapshot
                .state
                .update(cx, |state, cx| state.dismiss(window, cx));
            // A matching new kind cannot revive a trigger removed in an earlier frame.
            if focused
                && (snapshot.trigger_removed || self.plugin_indicator(cx) != Some(snapshot.trigger))
            {
                self.editor
                    .update(cx, |editor, cx| editor.focus(window, cx));
            }
        }
        cx.notify();
    }

    /// Merge startup, grammar and LSP tasks by validated ownership; unknown service state cannot route to a plugin.
    fn plugin_loading_groups(&self, cx: &App) -> BTreeMap<String, PluginLoadingSummary> {
        use crate::app::language_servers::ServiceLoadState;
        let panel = self.extensions.read(cx);
        let mut groups = BTreeMap::<String, PluginLoadingSummary>::new();
        let mut add = |plugin: Option<String>, fallback: String, detail: String| {
            let name = plugin
                .as_ref()
                .and_then(|id| panel.entries.iter().find(|entry| entry.manifest.id == *id))
                .map_or_else(|| fallback.clone(), |entry| entry.manifest.name.clone());
            let key = plugin
                .clone()
                .unwrap_or_else(|| format!("service:{fallback}"));
            groups
                .entry(key)
                .or_insert_with(|| PluginLoadingSummary {
                    plugin,
                    name,
                    details: Vec::new(),
                })
                .details
                .push(detail);
        };
        for (id, name) in &panel.startup {
            add(
                Some(id.clone()),
                name.clone(),
                t!("plugins.status_starting").to_string(),
            );
        }
        for (provider, state) in &self.dynamic_languages.entries {
            if matches!(state, Ok(false)) {
                add(
                    Some(provider.owner.clone()),
                    provider.owner.clone(),
                    t!(
                        "plugins.status_highlight_loading",
                        language = &provider.declaration.language
                    )
                    .to_string(),
                );
            }
        }
        let available = panel.language_services();
        let selected = crate::language::providers::language_servers();
        for (language, state) in &self.language_service_states {
            if *state != ServiceLoadState::Loading {
                continue;
            }
            let owned = self
                .language_servers
                .get(language)
                .and_then(|server| {
                    available.iter().find_map(|(key, plan)| {
                        plan.as_ref()
                            .ok()
                            .filter(|plan| server.uses_service(plan))
                            .map(|plan| (plan.owner.clone(), key.clone()))
                    })
                })
                .or_else(|| {
                    selected
                        .get(language)
                        .and_then(|key| key.as_ref())
                        .and_then(|key| {
                            panel
                                .entries
                                .iter()
                                .find(|entry| {
                                    entry.manifest.language_servers.iter().any(|provider| {
                                        format!("{}/{}", entry.manifest.id, provider.id) == *key
                                    })
                                })
                                .map(|entry| (entry.manifest.id.clone(), key.clone()))
                        })
                });
            let (plugin, service) = owned.map_or((None, language.clone()), |(plugin, service)| {
                (Some(plugin), service)
            });
            add(
                plugin,
                language.clone(),
                t!(
                    "plugins.status_service_loading",
                    language = language,
                    service = service
                )
                .to_string(),
            );
        }
        groups
    }
}

#[cfg(test)]
mod status_tests;
#[cfg(test)]
mod tests;
