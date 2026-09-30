//! Theme-aware diagnostic cards over Base's selectable text and the measured popup frame.

use gpui_base::input::{DiagnosticEntry, DiagnosticSeverity};
use gpui_kit::component::{ActiveTheme as _, v_flex};
use gpui_kit::{
    App, Bounds, FocusHandle, InteractiveElement, IntoElement, ParentElement, Pixels,
    StatefulInteractiveElement, Styled, div, px,
};
use rust_i18n::t;

/// Present the complete error and source location without clipping long messages.
pub(crate) fn diagnostic_popup(
    anchor: Bounds<Pixels>,
    diagnostic: &DiagnosticEntry,
    focus: &FocusHandle,
    dismiss: impl Fn(&mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    // Semantic diagnostics keep their server severity, source, and error code.
    let (title, color) = match diagnostic.severity {
        DiagnosticSeverity::Error => (t!("diagnostics.error"), cx.theme().danger),
        DiagnosticSeverity::Warning => (t!("diagnostics.warning"), cx.theme().warning),
        DiagnosticSeverity::Info => (t!("diagnostics.info"), cx.theme().primary),
        DiagnosticSeverity::Hint => (t!("diagnostics.hint"), cx.theme().muted_foreground),
    };
    let location = t!(
        "diagnostics.location",
        line = diagnostic.diagnostic.range.start.line + 1,
        column = diagnostic.diagnostic.range.start.character + 1
    )
    .to_string();
    let mut source = diagnostic
        .source
        .as_ref()
        .map(|source| format!("{source} · {location}"))
        .unwrap_or(location);
    if let Some(code) = &diagnostic.code {
        source.push_str(&format!(" · {code}"));
    }
    // Escape grammar tokens before Markdown parsing: punctuation in an error
    // message is content and must never become a link or HTML element.
    let mut message = String::new();
    for character in diagnostic.message.chars() {
        if "\\`*_{}[]<>()#+-.!|".contains(character) {
            message.push('\\');
        }
        message.push(character);
    }
    let content = v_flex()
        .id("editor-diagnostic-card")
        .debug_selector(|| "editor-diagnostic-card".into())
        .occlude()
        .track_focus(focus)
        .min_w(px(240.))
        .flex_1()
        .min_h_0()
        .p_3()
        .gap_2()
        .border_1()
        .border_color(color)
        .bg(cx.theme().popover)
        .text_color(cx.theme().popover_foreground)
        .on_mouse_move(|event, _, cx| {
            // Keep text selection usable without forwarding hover to the editor.
            if event.pressed_button.is_none() {
                cx.stop_propagation();
            }
        })
        // Selectable text can own keyboard focus; Escape must also work from that path.
        .on_key_down(move |event, _, cx| {
            if event.keystroke.key == "escape" {
                dismiss(cx);
                cx.stop_propagation();
            }
        })
        .child(div().text_color(color).child(title.to_string()))
        .child(
            div()
                .text_color(cx.theme().muted_foreground)
                .text_sm()
                .child(source),
        )
        .child(
            div()
                .id("editor-diagnostic-message")
                .min_h_0()
                .overflow_y_scroll()
                .child(super::markdown_view(
                    "editor-diagnostic-details",
                    message,
                    crate::ui::typography::editor_font_size(cx),
                    cx,
                )),
        );
    super::definition_popup(anchor, content.into_any_element())
}
