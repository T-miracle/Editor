//! Service selection uses local segmented controls and the same confirmed configuration channel.
use super::*;

impl SettingsView {
    pub(super) fn service_row(
        &mut self,
        choice: protocol::service::Choice,
        disabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let application = choice.scope == protocol::api::InstanceScope::Application;
        let key = format!(
            "{}-{}",
            if application {
                "application"
            } else {
                "workspace"
            },
            choice.contract
        );
        let scope = *self
            .service_scopes
            .entry(key.clone())
            .or_insert(Scope::User);
        let labels = std::iter::once(t!("settings.service_auto").to_string())
            .chain(choice.candidates.iter().cloned())
            .collect::<Vec<_>>();
        let explicit = if scope == Scope::Project {
            &choice.project
        } else {
            &choice.user
        };
        let selected = explicit
            .as_ref()
            .and_then(|id| choice.candidates.iter().position(|value| value == id))
            .map_or(0, |i| i + 1);
        let owner = choice.scope;
        let contract = choice.contract.clone();
        let options = choice.candidates;
        let view = cx.entity();
        let scope_view = cx.entity();
        let scope_key = key.clone();
        let selector = format!("service-provider-{key}");
        v_flex()
            .gap_2()
            .p_3()
            .border_1()
            .border_color(cx.theme().border)
            .rounded(cx.theme().radius)
            .child(div().font_semibold().child(format!(
                "{} · {}",
                t!("settings.service_provider"),
                choice.contract
            )))
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("settings.service_hint").to_string()),
            )
            .child(
                SegmentedTabs::new(SharedString::from(format!("{key}-scope")))
                    .labels([
                        t!("settings.plugin_user").to_string(),
                        t!("settings.plugin_project").to_string(),
                    ])
                    .selected_index(usize::from(scope == Scope::Project))
                    .disabled(disabled || application)
                    .on_change(move |index, _, cx| {
                        scope_view.update(cx, |this, cx| {
                            this.service_scopes.insert(
                                scope_key.clone(),
                                if index == 0 {
                                    Scope::User
                                } else {
                                    Scope::Project
                                },
                            );
                            cx.notify();
                        })
                    }),
            )
            .child(
                div().debug_selector(move || selector.clone()).child(
                    SegmentedTabs::new(SharedString::from(format!("{key}-providers")))
                        .labels(labels)
                        .selected_index(selected)
                        .disabled(disabled)
                        .on_change(move |index, _, cx| {
                            view.update(cx, |this, cx| {
                                let request = next_request();
                                let provider =
                                    index.checked_sub(1).map(|index| options[index].clone());
                                let result =
                                    this.owner
                                        .read(cx)
                                        .worker
                                        .tx
                                        .send(Work::SetServiceProvider {
                                            request,
                                            owner,
                                            scope,
                                            contract: contract.clone(),
                                            provider,
                                        });
                                this.failed = result.is_err();
                                if result.is_ok() {
                                    this.pending = Some(request);
                                    this.status = Some(t!("settings.plugin_applying").to_string());
                                } else {
                                    this.status =
                                        Some(t!("settings.plugin_worker_unavailable").to_string());
                                }
                                cx.notify();
                            })
                        }),
                ),
            )
            .into_any_element()
    }
}
