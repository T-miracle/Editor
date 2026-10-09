//! Native editor appearance is projected onto any existing entity without changing its text or behavior.
use super::{Button, Icon};
use crate::theme::component_styles;
use gpui_base::InputBase;
use gpui_base::input::{DiagnosticColors, Editor as BaseEditor, EditorState, InputEditorStyle};
use gpui_kit::component::{ActiveTheme as _, IconName};
use gpui_kit::{
    AccessibleAction, AnyElement, App, Entity, Focusable as _, InteractiveElement, IntoElement,
    ParentElement, Role, StatefulInteractiveElement, Styled, Window, prelude::FluentBuilder as _,
    px,
};
use plugin_schema::ThemeComponent;
use std::rc::Rc;

/// Expose a readonly native document's source name, current value and actual editing focus.
/// The Base editor still owns input and layout. Document is a readonly accessibility role;
/// GPUI's input frame has no public readonly flag, so TextInput would falsely report writable.
/// No SetValue action is registered: assistive clients cannot obtain another editing path.
pub(crate) fn readonly_editor(
    id: &'static str,
    label: String,
    editor: &Entity<EditorState>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let focus = editor.read(cx).focus_handle(cx);
    // Mirror the component's lazy accessibility value; ordinary painting need not clone text.
    let value =
        (window.is_a11y_active() || cfg!(test)).then(|| editor.read(cx).value().to_string());
    let target = editor.clone();
    InputBase::new(id)
        .role(Role::Document)
        .accessibility_id(id)
        .accessibility_label(label)
        .track_focus(&focus)
        .focused(focus.is_focused(window))
        .when_some(value, |frame, value| frame.aria_value(value))
        .on_a11y_action(AccessibleAction::Focus, move |_, window, cx| {
            target.update(cx, |state, cx| state.focus(window, cx));
        })
        .flex()
        .size_full()
        .min_h_0()
        .child(BaseEditor::new(editor))
        .into_any_element()
}

/// Refresh palette, highlight resolver and folding glyphs for active or comparison editors.
/// Base continues to own selection, IME, scroll, focus and folds; no document state is copied.
pub(crate) fn synchronize_editor_appearance(editor: &Entity<EditorState>, cx: &mut App) {
    let palette = cx.theme();
    let appearance = component_styles(cx, ThemeComponent::Editor).base;
    let style = InputEditorStyle {
        foreground: appearance.foreground.unwrap_or(palette.foreground),
        muted_foreground: palette.muted_foreground,
        background: appearance.background.unwrap_or(palette.background),
        border: palette.border,
        selection: palette.selection,
        caret: palette.caret,
        diagnostics: DiagnosticColors {
            error: palette.highlight_theme.style.status.error(cx),
            warning: palette.highlight_theme.style.status.warning(cx),
            info: palette.highlight_theme.style.status.info(cx),
            hint: palette.highlight_theme.style.status.hint(cx),
        },
        highlight_styles: palette.highlight_theme.clone(),
        editor_invisible: palette.highlight_theme.style.editor_invisible,
        editor_active_line: palette.highlight_theme.style.editor_active_line,
        editor_gutter_background: palette.highlight_theme.style.editor_gutter_background,
        fold_icon_renderer: Some(Rc::new(|index, folded| {
            Button::new(("document-fold", index))
                .ghost()
                .small()
                .compact()
                .icon(
                    Icon::new(if folded {
                        IconName::ChevronRight
                    } else {
                        IconName::ChevronDown
                    })
                    .xsmall(),
                )
                .w(px(14.))
                .h(px(14.))
                .into_any_element()
        })),
    };
    editor.update(cx, |state, _| state.set_editor_style(style));
}
