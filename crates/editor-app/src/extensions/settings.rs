//! Native plugin forms reuse editor-owned controls; confirmations cross the existing worker boundary.
use super::*;
use crate::ui::controls::{Checkbox, SegmentedTabs};
use protocol::settings::{Definition, EffectiveValue, Scope, SettingType, Source};
use std::collections::BTreeSet;

/// Drafts are UI state only; the worker remains the sole owner of validated persistent configuration.
struct Draft {
    scope: Scope,
    value: serde_json::Value,
    input: Entity<InputState>,
    effective: EffectiveValue,
}

pub(crate) struct SettingsView {
    owner: Entity<ExtensionPanel>,
    drafts: BTreeMap<String, Draft>,
    pending: Option<u64>,
    status: Option<String>,
    failed: bool,
    _subscription: Subscription,
}

impl SettingsView {
    /// Observe publication rather than executing or loading plugins on the UI thread.
    pub(crate) fn new(owner: Entity<ExtensionPanel>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&owner, |_, _, cx| cx.notify());
        Self {
            owner,
            drafts: Default::default(),
            pending: None,
            status: None,
            failed: false,
            _subscription: subscription,
        }
    }

    /// Apply is the explicit project confirmation; parse errors leave both effective state and disk untouched.
    fn submit(
        &mut self,
        plugin: String,
        key: String,
        definition: Definition,
        reset: bool,
        cx: &mut Context<Self>,
    ) {
        if self.pending.is_some() {
            return;
        }
        let draft = &self.drafts[&format!("{plugin}/{key}")];
        let value = if reset {
            None
        } else {
            let value = match definition.value_type {
                SettingType::String { .. } => {
                    serde_json::Value::String(draft.input.read(cx).value().to_string())
                }
                SettingType::Integer { .. } => match draft.input.read(cx).value().parse::<i64>() {
                    Ok(value) => serde_json::json!(value),
                    Err(_) => {
                        self.status = Some(t!("settings.plugin_invalid_integer").to_string());
                        self.failed = true;
                        cx.notify();
                        return;
                    }
                },
                _ => draft.value.clone(),
            };
            if !definition.accepts(&value) {
                self.status = Some(t!("settings.plugin_invalid_value").to_string());
                self.failed = true;
                cx.notify();
                return;
            }
            Some(value)
        };
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let request = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let result = self.owner.read(cx).worker.tx.send(Work::SetSetting {
            request,
            plugin,
            scope: draft.scope,
            key,
            value,
        });
        self.failed = result.is_err();
        if result.is_ok() {
            self.pending = Some(request);
            self.status = Some(t!("settings.plugin_applying").to_string());
        } else {
            self.status = Some(t!("settings.plugin_worker_unavailable").to_string());
        }
        cx.notify();
    }

    /// Render one declaration with a local draft; host publications cannot overwrite unsaved input on every frame.
    fn field(
        &mut self,
        plugin: &str,
        key: &str,
        definition: &Definition,
        effective: EffectiveValue,
        application: bool,
        disabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let draft_key = format!("{plugin}/{key}");
        let selector = format!("setting-{plugin}-{key}");
        let text = |value: &serde_json::Value| {
            value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string())
        };
        let draft = self
            .drafts
            .entry(draft_key.clone())
            .or_insert_with(|| Draft {
                scope: Scope::User,
                value: effective.value.clone(),
                input: cx
                    .new(|cx| InputState::new(window, cx).default_value(text(&effective.value))),
                effective: effective.clone(),
            });
        if draft.effective != effective {
            draft.value = effective.value.clone();
            draft.input.update(cx, |input, cx| {
                input.set_value(text(&effective.value), window, cx)
            });
            draft.effective = effective.clone();
        }
        let value_control = match &definition.value_type {
            SettingType::Boolean => {
                let view = cx.entity();
                let field = draft_key.clone();
                Checkbox::new(SharedString::from(format!("{selector}-checkbox")))
                    .checked(draft.value.as_bool().unwrap_or(false))
                    .disabled(disabled)
                    .on_change(move |value, _, cx| {
                        view.update(cx, |view, cx| {
                            view.drafts.get_mut(&field).unwrap().value = serde_json::json!(value);
                            cx.notify();
                        })
                    })
                    .into_any_element()
            }
            SettingType::Enum { choices } => {
                let view = cx.entity();
                let field = draft_key.clone();
                let values = choices.clone();
                SegmentedTabs::new(SharedString::from(format!("{selector}-choices")))
                    .labels(choices.clone())
                    .selected_index(
                        choices
                            .iter()
                            .position(|value| Some(value.as_str()) == draft.value.as_str())
                            .unwrap_or(0),
                    )
                    .disabled(disabled)
                    .on_change(move |index, _, cx| {
                        view.update(cx, |view, cx| {
                            view.drafts.get_mut(&field).unwrap().value =
                                serde_json::json!(values[index]);
                            cx.notify();
                        })
                    })
                    .into_any_element()
            }
            _ => Input::new(&draft.input).into_any_element(),
        };
        let view = cx.entity();
        let field = draft_key;
        let scope_control = if definition.scope == Scope::Project && !application {
            SegmentedTabs::new(SharedString::from(format!("{selector}-scope")))
                .labels([
                    t!("settings.plugin_user").to_string(),
                    t!("settings.plugin_project").to_string(),
                ])
                .selected_index(usize::from(draft.scope == Scope::Project))
                .disabled(disabled)
                .on_change(move |index, _, cx| {
                    view.update(cx, |view, cx| {
                        view.drafts.get_mut(&field).unwrap().scope = if index == 0 {
                            Scope::User
                        } else {
                            Scope::Project
                        };
                        cx.notify();
                    })
                })
                .into_any_element()
        } else {
            div()
                .text_sm()
                .child(t!("settings.plugin_user_only").to_string())
                .into_any_element()
        };
        let source = match effective.source {
            Source::Default => "default",
            Source::Discovered => "discovered",
            Source::User => "user",
            Source::Project => "project",
        };
        let source_selector = format!("{selector}-source-{source}");
        let source_key = format!("settings.plugin_source_{source}");
        let source_label = t!(&source_key).to_string();
        let value_selector = format!("{selector}-value");
        let apply_selector = format!("{selector}-apply");
        let reset_selector = format!("{selector}-reset");
        let apply_view = cx.entity();
        let apply_plugin = plugin.to_owned();
        let apply_key = key.to_owned();
        let apply_definition = definition.clone();
        let reset_view = cx.entity();
        let reset_plugin = plugin.to_owned();
        let reset_key = key.to_owned();
        let reset_definition = definition.clone();
        v_flex()
            .gap_2()
            .p_3()
            .border_1()
            .border_color(cx.theme().border)
            .rounded(cx.theme().radius)
            .child(
                h_flex()
                    .justify_between()
                    .child(definition.title.clone())
                    .child(
                        div()
                            .debug_selector(move || source_selector.clone())
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(source_label),
                    ),
            )
            .child(
                div()
                    .debug_selector(move || value_selector.clone())
                    .child(value_control),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(scope_control)
                    .child(
                        div().debug_selector(move || apply_selector.clone()).child(
                            Button::new(SharedString::from(format!("{selector}-apply-button")))
                                .label(t!("settings.plugin_apply").to_string())
                                .small()
                                .disabled(disabled)
                                .on_click(move |_, _, cx| {
                                    apply_view.update(cx, |view, cx| {
                                        view.submit(
                                            apply_plugin.clone(),
                                            apply_key.clone(),
                                            apply_definition.clone(),
                                            false,
                                            cx,
                                        )
                                    })
                                }),
                        ),
                    )
                    .child(
                        div().debug_selector(move || reset_selector.clone()).child(
                            Button::new(SharedString::from(format!("{selector}-reset-button")))
                                .label(t!("settings.plugin_reset").to_string())
                                .small()
                                .disabled(disabled)
                                .on_click(move |_, _, cx| {
                                    reset_view.update(cx, |view, cx| {
                                        view.submit(
                                            reset_plugin.clone(),
                                            reset_key.clone(),
                                            reset_definition.clone(),
                                            true,
                                            cx,
                                        )
                                    })
                                }),
                        ),
                    ),
            )
            .into_any_element()
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let owner = self.owner.read(cx);
        let entries = owner.entries.clone();
        let disabled = !owner
            .worker
            .trusted
            .load(std::sync::atomic::Ordering::Acquire);
        let (configurations, result) = {
            let state = owner.worker.state.lock().unwrap();
            (
                state.configurations.clone(),
                state.configuration_result.clone(),
            )
        };
        if let Some((request, result)) =
            result.filter(|(request, _)| Some(*request) == self.pending)
        {
            let _ = request;
            self.pending = None;
            self.failed = result.is_err();
            self.status = Some(match result {
                Ok(()) => t!("settings.plugin_applied").to_string(),
                Err(error) => error,
            });
        }
        let mut rows = Vec::new();
        let mut retained = BTreeSet::new();
        for entry in entries {
            if entry.manifest.settings.is_empty() {
                continue;
            }
            rows.push(
                div()
                    .font_semibold()
                    .child(entry.manifest.name.clone())
                    .into_any_element(),
            );
            if let Some(Err(error)) = configurations.get(&entry.manifest.id) {
                rows.push(
                    div()
                        .text_color(cx.theme().danger)
                        .child(error.clone())
                        .into_any_element(),
                );
            }
            for (key, definition) in &entry.manifest.settings {
                retained.insert(format!("{}/{key}", entry.manifest.id));
                let value = configurations
                    .get(&entry.manifest.id)
                    .and_then(|result| result.as_ref().ok())
                    .and_then(|values| values.get(key))
                    .cloned()
                    .unwrap_or_else(|| EffectiveValue {
                        value: definition.default.clone(),
                        source: Source::Default,
                    });
                rows.push(self.field(
                    &entry.manifest.id,
                    key,
                    definition,
                    value,
                    entry.manifest.scope == protocol::api::InstanceScope::Application,
                    disabled || self.pending.is_some(),
                    window,
                    cx,
                ));
            }
        }
        self.drafts.retain(|key, _| retained.contains(key));
        v_flex()
            .gap_3()
            .w_full()
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("settings.plugin_hint").to_string()),
            )
            .when(disabled, |view| {
                view.child(t!("settings.plugin_restricted").to_string())
            })
            .when_some(self.status.clone(), |view, status| {
                view.child(
                    div()
                        .debug_selector({
                            let failed = self.failed;
                            move || {
                                if failed {
                                    "plugin-settings-error".into()
                                } else {
                                    "plugin-settings-status".into()
                                }
                            }
                        })
                        .child(status),
                )
            })
            .when(rows.is_empty(), |view| {
                view.child(t!("settings.plugin_empty").to_string())
            })
            .children(rows)
    }
}
