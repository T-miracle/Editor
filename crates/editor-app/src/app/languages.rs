//! Hot grammar tasks publish only into the still-selected generation, then refresh open documents.
use crate::language::providers::{self, GrammarProvider};
use crate::*;
use gpui_kit::AnyElement;

/// The current request owns status; stale worker results never replace this state or global parsers.
#[derive(Default)]
pub(crate) struct DynamicLanguages {
    pub entries: Vec<(GrammarProvider, Result<bool, String>)>,
    generation: u64,
    scope: plugin_runtime::plugin_protocol::settings::Scope,
    error: Option<String>,
}

/// Reuse native host controls for independent recognition and grammar preferences.
pub(crate) fn render_settings(view: &Entity<EditorApp>, cx: &App) -> AnyElement {
    use crate::ui::controls::SegmentedTabs;
    use plugin_runtime::plugin_protocol::settings::Scope;
    let scope = view.read(cx).dynamic_languages.scope;
    let trusted = view.read(cx).session_state.workspace_trusted;
    let owner = view.clone();
    let mut content = v_flex().gap_4().child(
        div().debug_selector(|| "provider-scope".into()).child(
            SegmentedTabs::new("provider-scope-tabs")
                .labels([
                    t!("settings.provider_scope_user").to_string(),
                    t!("settings.provider_scope_project").to_string(),
                ])
                .selected_index(usize::from(scope == Scope::Project))
                .on_change(move |index, _, cx| {
                    owner.update(cx, |app, cx| {
                        app.dynamic_languages.scope = if index == 0 {
                            Scope::User
                        } else {
                            Scope::Project
                        };
                        app.refresh_dialog(cx);
                    })
                }),
        ),
    );
    let rows = providers::rows();
    if rows.is_empty() {
        content = content.child(t!("settings.providers_empty").to_string());
    }
    for row in rows {
        let status = if row.selected.is_some() {
            t!(format!("settings.provider_source_{}", row.source)).to_string()
        } else if row.candidates.is_empty() {
            t!("settings.provider_missing").to_string()
        } else {
            t!("settings.provider_choose").to_string()
        };
        let role = if row.key.starts_with("lsp:") {
            "LSP".into()
        } else if row.key.starts_with("highlight:") {
            t!("settings.provider_highlight")
        } else {
            t!("settings.provider_recognition")
        };
        let mut options = v_flex()
            .gap_2()
            .child(format!("{role} · {}", row.key.split_once(':').unwrap().1))
            .child(
                div()
                    .debug_selector({
                        let selector = if row.selected.is_some() {
                            format!("provider-source-{}-{}", row.source, row.key)
                        } else if row.candidates.is_empty() {
                            format!("provider-unavailable-{}", row.key)
                        } else {
                            format!("provider-choice-needed-{}", row.key)
                        };
                        move || selector.clone()
                    })
                    .child(status),
            );
        for candidate in row.candidates {
            let selected = row.selected.as_ref() == Some(&candidate);
            let selector = format!("provider-{}-{candidate}", row.key);
            let selected_selector = format!("provider-selected-{}-{candidate}", row.key);
            let key = row.key.clone();
            let owner = view.clone();
            let button = Button::new(selector.clone())
                .label(candidate.clone())
                .outline()
                .disabled(!trusted)
                .when(selected, |button| button.primary())
                .on_click(move |_, _, cx| select(&owner, scope, &key, Some(&candidate), cx));
            options = options.child(
                div().debug_selector(move || selector.clone()).child(
                    div()
                        .when(selected, |d| {
                            d.debug_selector(move || selected_selector.clone())
                        })
                        .child(button),
                ),
            );
        }
        let owner = view.clone();
        let key = row.key.clone();
        options = options.child(
            Button::new(format!("provider-reset-{}", row.key))
                .label(t!("settings.plugin_reset").to_string())
                .small()
                .disabled(!trusted)
                .on_click(move |_, _, cx| select(&owner, scope, &key, None, cx)),
        );
        content = content.child(options);
    }
    for (provider, state) in &view.read(cx).dynamic_languages.entries {
        let status = match state {
            Ok(true) => continue,
            Ok(false) => t!("plugins.loading").to_string(),
            Err(error) => error.clone(),
        };
        content = content.child(
            div()
                .debug_selector(|| "provider-load-status".into())
                .child(format!("{}: {status}", provider.declaration.language)),
        );
    }
    if let Some(error) = view
        .read(cx)
        .dynamic_languages
        .error
        .clone()
        .or_else(providers::error)
    {
        content = content.child(
            div()
                .debug_selector(|| "provider-error".into())
                .text_color(cx.theme().danger)
                .child(error),
        );
    }
    content.into_any_element()
}

/// A confirmed provider click changes only language policy; it grants no execution authority.
fn select(
    view: &Entity<EditorApp>,
    scope: plugin_runtime::plugin_protocol::settings::Scope,
    key: &str,
    provider: Option<&str>,
    cx: &mut App,
) {
    view.update(cx, |app, cx| {
        if !app.session_state.workspace_trusted {
            return;
        }
        app.dynamic_languages.error = providers::choose(scope, key, provider)
            .err()
            .map(|e| format!("{e:#}"));
        if app.dynamic_languages.error.is_none() {
            app.sync_dynamic_languages(cx);
        }
        app.refresh_dialog(cx);
        cx.notify();
    });
}

impl EditorApp {
    pub(crate) fn sync_dynamic_languages(&mut self, cx: &mut Context<Self>) {
        self.plugin_loading_generation = self.plugin_loading_generation.wrapping_add(1);
        self.sync_dynamic_language_servers(cx);
        self.dynamic_languages.generation += 1;
        let generation = self.dynamic_languages.generation;
        let selected = providers::grammars();
        let old = std::mem::take(&mut self.dynamic_languages.entries);
        for language in providers::languages() {
            if !selected.iter().any(|p| p.declaration.language == language) {
                // Recognition alone must never accidentally enable an upstream built-in grammar.
                language_plugins::mask_language(&language);
            }
        }
        for (provider, _) in &old {
            if !selected.contains(provider) {
                language_plugins::mask_language(&provider.declaration.language);
            }
        }
        for provider in selected {
            if let Some((_, Ok(true))) = old.iter().find(|(old, _)| old == &provider) {
                self.dynamic_languages.entries.push((provider, Ok(true)));
                continue;
            }
            let language = provider.declaration.language.clone();
            language_plugins::mask_language(&language);
            self.dynamic_languages
                .entries
                .push((provider.clone(), Ok(false)));
            let logs = self.extensions.read(cx).runtime_logs();
            let source = format!("language.grammar:{}", provider.declaration.language);
            logs.append(
                &provider.owner,
                plugin_runtime::logs::LogLevel::Info,
                &source,
                t!("plugins.logs.grammar_loading").to_string(),
            );
            cx.spawn(async move |this, cx| {
                let loading = provider.clone();
                let result = cx
                    .background_executor()
                    .spawn(async move { language_plugins::prepare_dynamic(&loading) })
                    .await;
                let _ = this.update(cx, |app, cx| {
                    if app.dynamic_languages.generation != generation
                        || !providers::is_current(&provider)
                    {
                        return;
                    }
                    let Some((_, state)) = app
                        .dynamic_languages
                        .entries
                        .iter_mut()
                        .find(|(p, _)| p == &provider)
                    else {
                        return;
                    };
                    // Attribution and generation are checked before publishing either resources or diagnostics.
                    let (level, message) = match &result {
                        Ok(_) => (
                            plugin_runtime::logs::LogLevel::Info,
                            t!("plugins.logs.grammar_ready").to_string(),
                        ),
                        Err(error) => (plugin_runtime::logs::LogLevel::Error, format!("{error:#}")),
                    };
                    logs.append(&provider.owner, level, &source, message);
                    *state = match result {
                        Ok((grammar, query)) => {
                            language_plugins::publish_dynamic(&language, grammar, query);
                            Ok(true)
                        }
                        Err(error) => Err(format!("{error:#}")),
                    };
                    app.refresh_dynamic_documents(cx);
                    app.refresh_dialog(cx);
                    cx.notify();
                });
            })
            .detach();
        }
        // Withdrawing a provider immediately revokes old highlighting while replacement loads.
        self.refresh_dynamic_documents(cx);
    }

    fn refresh_dynamic_documents(&mut self, cx: &mut Context<Self>) {
        let current = providers::languages();
        for tab in &self.tabs {
            let language = editor::language_for_path(tab.session.path());
            if providers::handles_path(tab.session.path())
                || current.contains(&language)
                || self
                    .dynamic_language_ids
                    .contains(tab.editor.read(cx).language_name().as_ref())
            {
                // Highlight-only providers keep the identity supplied by an independent recognizer.
                tab.editor
                    .update(cx, |editor, cx| editor.set_highlighter(language, cx));
            }
        }
        self.dynamic_language_ids = current;
        self.reset_syntax_diagnostics(cx);
        cx.notify();
    }
}
