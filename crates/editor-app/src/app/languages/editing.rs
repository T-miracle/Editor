//! Native editing preferences are user policy with independent per-language inheritance.
use crate::language::providers;
use crate::ui::controls::Checkbox;
use crate::*;

/// Use the same local Base checkbox behavior as the remaining settings dialog.
pub(super) fn render(view: &Entity<EditorApp>, _cx: &App) -> gpui_kit::AnyElement {
    let mut content = v_flex().gap_2();
    let languages = std::iter::once("*".to_string()).chain(providers::languages());
    for language in languages {
        let values = providers::editing_preferences(&language);
        let label = if language == "*" {
            t!("settings.editing_default").to_string()
        } else if providers::has_editing_override(&language) {
            format!("{language} · {}", t!("settings.editing_override"))
        } else {
            language.clone()
        };
        let mut row = v_flex().gap_2().child(label);
        for (formatting, checked, key) in [
            (true, values.format_on_save, "settings.format_on_save"),
            (false, values.linked_editing, "settings.linked_editing"),
        ] {
            let owner = view.clone();
            let language = language.clone();
            row = row.child(
                div()
                    .debug_selector({
                        let selector = format!("editing-setting-{language}-{formatting}");
                        move || selector.clone()
                    })
                    .child(
                        Checkbox::new(format!("editing-{language}-{formatting}"))
                            .label(t!(key).to_string())
                            .checked(checked)
                            .on_change(move |value, _, cx| {
                                owner.update(cx, |app, cx| {
                                    app.dynamic_languages.error =
                                        providers::set_editing_preference(
                                            &language,
                                            formatting,
                                            Some(*value),
                                        )
                                        .err()
                                        .map(|error| format!("{error:#}"));
                                    app.sync_linked_input(cx);
                                    app.refresh_dialog(cx);
                                    cx.notify();
                                });
                            }),
                    ),
            );
        }
        if language != "*" {
            let owner = view.clone();
            let language = language.clone();
            row = row.child(
                Button::new(format!("editing-reset-{language}"))
                    .label(t!("settings.editing_reset").to_string())
                    .small()
                    .on_click(move |_, _, cx| {
                        owner.update(cx, |app, cx| {
                            app.dynamic_languages.error =
                                providers::set_editing_preference(&language, false, None)
                                    .err()
                                    .map(|error| format!("{error:#}"));
                            app.sync_linked_input(cx);
                            app.refresh_dialog(cx);
                            cx.notify();
                        });
                    }),
            );
        }
        content = content.child(row);
    }
    content.into_any_element()
}
