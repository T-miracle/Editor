//! Native file-association settings share the language registry and its installed providers.
use super::*;
use crate::ui::controls::Input;
use gpui_base::input::InputState;

/// User associations live outside project content and never enable a plugin or grant trust.
pub(super) fn render(view: &Entity<EditorApp>, cx: &App) -> AnyElement {
    let owner = view.clone();
    let mut content = v_flex()
        .gap_2()
        .child(t!("settings.file_associations").to_string())
        .child(
            Button::new("file-association-add")
                .debug_selector(|| "file-association-add".into())
                .label(t!("settings.file_association_add").to_string())
                .small()
                .on_click(move |_, window, cx| {
                    owner.update(cx, |app, cx| {
                        app.dynamic_languages.extension_input = Some(cx.new(|cx| {
                            InputState::new(window, cx)
                                .placeholder(t!("settings.file_association_extension").to_string())
                        }));
                        app.refresh_dialog(cx);
                    });
                }),
        );
    for (extension, language) in providers::file_associations() {
        let owner = view.clone();
        let key = extension.clone();
        content = content.child(
            h_flex()
                .gap_2()
                .child(format!(".{extension} → {language}"))
                .child(
                    Button::new(format!("file-association-remove-{extension}"))
                        .label(t!("settings.plugin_reset").to_string())
                        .small()
                        .on_click(move |_, _, cx| apply(&owner, &key, None, cx)),
                ),
        );
    }
    if let Some(input) = view.read(cx).dynamic_languages.extension_input.clone() {
        content = content.child(
            div()
                .debug_selector(|| "file-association-extension".into())
                .child(Input::new(&input)),
        );
        let mut choices = h_flex().gap_2().flex_wrap();
        for language in providers::languages() {
            let owner = view.clone();
            let input = input.clone();
            let selector = format!("file-association-language-{language}");
            choices = choices.child(
                Button::new(format!("file-association-language-{language}"))
                    .debug_selector(move || selector.clone())
                    .label(language.clone())
                    .small()
                    .on_click(move |_, _, cx| {
                        let extension = input.read(cx).value().to_string();
                        apply(&owner, &extension, Some(&language), cx);
                    }),
            );
        }
        content = content.child(choices);
    }
    content.into_any_element()
}

/// Persisting a setting changes only recognition; language activation still observes trust.
fn apply(view: &Entity<EditorApp>, extension: &str, language: Option<&str>, cx: &mut App) {
    view.update(cx, |app, cx| {
        app.dynamic_languages.error = providers::associate_extension(extension, language)
            .err()
            .map(|error| format!("{}: {error:#}", t!("settings.file_association_error")));
        if app.dynamic_languages.error.is_none() {
            app.dynamic_languages.extension_input = None;
            app.sync_dynamic_languages(cx);
        }
        app.refresh_dialog(cx);
        cx.notify();
    });
}
