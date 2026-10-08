//! Native leaf controls draw without retaining the recursive container/editor renderer on the stack.
use super::*;
use crate::ui::controls::{Button, ButtonCustomVariant, Input, Tooltip};
use gpui_base::{Checkbox, CheckboxState, Progress, Radio, RadioGroup};
use gpui_kit::{
    AnyElement, InteractiveElement, ParentElement, SharedString, StatefulInteractiveElement,
    Styled, StyledImage as _, div, prelude::FluentBuilder as _, px, relative,
};
use plugin_runtime::plugin_protocol::ui::Node;

impl PluginView {
    /// Concrete controls share the current scene gate, local appearance and Base interaction behavior.
    pub(super) fn leaf_node(
        &mut self,
        node: &Node,
        disabled: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = self.colors(node.theme_role(), cx);
        let id = node.id.clone();
        let native_id = SharedString::from(format!("plugin-ui-{}", node.id));
        match &node.kind {
            Kind::SideTabs(_) => self.collections[&node.id].clone().into_any_element(),
            Kind::Canvas(_) => self.canvases[&node.id].clone().into_any_element(),
            Kind::FileImage { alt, sizing } => {
                self.render_file_image(&node.id, alt, *sizing, node.viewport.clone(), cx)
            }
            Kind::Text { text } => div().child(text.clone()).into_any_element(),
            Kind::Image { source, alt } => {
                // Direct retained pixels bypass GPUI's ambient file/URL loader. Height follows the image ratio.
                let photo = self.photos.get(&node.id).filter(|photo| {
                    self.document
                        .source
                        .as_ref()
                        .is_some_and(|source| photo.resource.source == *source)
                        && source == &photo.resource.uri
                });
                if let Some(photo) = photo
                    && let Ok(Some(bitmap)) = &photo.decoded
                {
                    if node.viewport.is_some() {
                        return self.render_visual_bitmap(
                            &node.id,
                            bitmap.clone(),
                            ui::ImageSizing::OriginalContain,
                            node.viewport.clone(),
                            cx,
                        );
                    }
                    let image_id = format!("plugin-image-{}", node.id);
                    let pixels_id = format!("plugin-image-pixels-{}", node.id);
                    div()
                        .debug_selector(move || image_id.clone())
                        .flex()
                        .flex_col()
                        .w_full()
                        .min_w_0()
                        .child(
                            div()
                                .debug_selector(move || pixels_id.clone())
                                .flex_shrink_0()
                                .w_full()
                                .max_w(px(bitmap.width as f32))
                                .aspect_ratio(bitmap.width as f32 / bitmap.height as f32)
                                .child(
                                    gpui_kit::img(bitmap.image.clone())
                                        .size_full()
                                        .object_fit(gpui_kit::ObjectFit::Contain),
                                ),
                        )
                        .when(!alt.is_empty(), |view| {
                            view.child(div().text_size(px(12.)).child(alt.clone()))
                        })
                        .into_any_element()
                } else {
                    let key = super::bitmap::status_key(
                        photo.and_then(|photo| photo.decoded.as_ref().err()),
                    );
                    let label = rust_i18n::t!(key, locale = self.environment.locale.as_str());
                    let status_id = format!("plugin-image-status-{}-{key}", node.id);
                    div()
                        .debug_selector(move || status_id.clone())
                        .w_full()
                        .border_1()
                        .border_color(colors.border)
                        .rounded(px(4.))
                        .p_2()
                        .child(if alt.is_empty() {
                            label.to_string()
                        } else {
                            format!("{alt} · {label}")
                        })
                        .into_any_element()
                }
            }
            Kind::RichText { html } => {
                let owner = cx.entity().downgrade();
                let revision = self.document.revision;
                let link_events = self.document.link_events;
                crate::ui::controls::rich_text_view(
                    SharedString::from(format!("{native_id}-scene-{revision}")),
                    html.clone(),
                    px(self
                        .environment
                        .font_style(&self.plugin, node.theme_role(), false)
                        .size_px
                        .unwrap_or(14.)),
                    crate::ui::controls::RichTextColors {
                        foreground: colors.foreground,
                        muted_foreground: colors.muted_foreground,
                        code_background: colors.code_background,
                        inline_code_background: colors.inline_code_background,
                        border: colors.border,
                        link: colors.accent,
                    },
                    cx,
                )
                // Base resolves real link hit targets and selection gestures. No URI is opened here;
                // a negotiated event reaches the guest and then the versioned native effect boundary.
                .on_link_click(move |uri, event, _, cx| {
                    if link_events {
                        let _ = owner.update(cx, |this, cx| {
                            if this.accept_link_click(&id, revision, event) {
                                this.emit_version(
                                    &id,
                                    revision,
                                    Action::Link {
                                        uri: uri.to_string(),
                                    },
                                    cx,
                                );
                            }
                        });
                    }
                })
                .into_any_element()
            }
            Kind::CodeBlock { text, .. } => {
                // Each literal line keeps its whitespace; the host never parses guest code as markup.
                let font = self
                    .environment
                    .font_style(&self.plugin, node.theme_role(), true);
                div()
                    .id(native_id.clone())
                    .w_full()
                    .overflow_x_scroll()
                    .bg(colors.code_background)
                    .border_1()
                    .border_color(colors.border)
                    .rounded(px(4.))
                    .p_3()
                    .font_family(font.family.unwrap_or_else(|| "Consolas".into()))
                    .text_size(px(font.size_px.unwrap_or(14.)))
                    .child(div().flex().flex_col().children({
                        let mut start = 0;
                        text.split_inclusive('\n')
                            .enumerate()
                            .map(|(index, raw)| {
                                // CRLF consumes two source bytes; splitting offsets must retain both of them.
                                let line = raw
                                    .strip_suffix('\n')
                                    .map(|line| line.strip_suffix('\r').unwrap_or(line))
                                    .unwrap_or(raw);
                                let content = self.code_line(&node.id, index, line, start, cx);
                                start += raw.len();
                                content
                            })
                            .collect::<Vec<_>>()
                    }))
                    .into_any_element()
            }
            Kind::Button { label } => self
                .font(
                    Button::new(native_id.clone())
                        .debug_selector({
                            // Keep native form actions addressable after splitting the leaf renderer.
                            let control = format!("plugin-button-{}", node.id);
                            move || control.clone()
                        })
                        .accessibility_label(label.clone())
                        .when(node.button_icon.is_none(), |button| {
                            button.label(label.clone())
                        })
                        .when_some(node.button_icon.as_ref(), |button, svg| {
                            // The negotiated, validated icon retains Base focus and Click behavior.
                            button
                                .icon(crate::ui::controls::Icon::default().data(svg.as_bytes()))
                                .small()
                                .compact()
                        })
                        .when_some(node.tooltip.clone(), |button, tooltip| {
                            button.accessibility_label(tooltip.clone()).tooltip(tooltip)
                        })
                        .disabled(disabled)
                        .border_color(colors.border)
                        .custom(
                            ButtonCustomVariant::new(cx)
                                .color(colors.background)
                                .foreground(colors.foreground)
                                .hover(colors.hover)
                                .active(colors.active),
                        )
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.emit(&id, Action::Click, cx)),
                        ),
                    node.theme_role(),
                )
                .into_any_element(),
            Kind::Input(_) => self
                .font(
                    Input::new(&self.inputs[&node.id].state)
                        .bg(colors.background)
                        .text_color(colors.foreground)
                        .border_color(colors.border),
                    node.theme_role(),
                )
                .into_any_element(),
            // Multiline form fields share the stable state and revision rules of single-line input.
            Kind::Textarea(_) => self
                .font(
                    div().w_full().child(crate::ui::controls::Textarea::new(
                        &self.textareas[&node.id].state,
                    )),
                    node.theme_role(),
                )
                .into_any_element(),
            Kind::Checkbox { label, checked } => {
                let owner = cx.entity().downgrade();
                let revision = self.document.revision;
                // Base retains a press by element ID. A fresh scene must not consume a prior press,
                // while an explicit focus handle lets ordinary task edits keep keyboard activation.
                let gesture_id = SharedString::from(format!("{native_id}-scene-{revision}"));
                // Marker-only controls keep their prose outside the checkbox. The ordinary
                // localized hint supplies an accessible name without duplicating visible text.
                let accessible_label = if label.is_empty() {
                    node.tooltip.as_deref().unwrap_or(label).to_owned()
                } else {
                    label.clone()
                };
                let marker_id = format!("plugin-checkbox-marker-{}", node.id);
                let marker = div()
                    // Expose the visible hit target separately from a stretched list-row wrapper.
                    .debug_selector(move || marker_id.clone())
                    .size(px(16.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .border_1()
                    .border_color(colors.border)
                    .rounded(px(3.))
                    .bg(if *checked {
                        colors.accent
                    } else {
                        colors.background
                    })
                    .text_color(colors.accent_foreground)
                    .when(*checked, |v| v.child("✓"));
                self.font(
                    Checkbox::new(gesture_id)
                        .track_focus(&self.checkbox_focus[&node.id])
                        .checked(*checked)
                        .disabled(disabled)
                        .accessibility_label(accessible_label)
                        // Base owns traversal and activation; the editor supplies its focus appearance.
                        .border_1()
                        .border_color(colors.background)
                        .rounded(px(4.))
                        .focus_visible(|style| style.border_color(colors.accent))
                        .when_some(node.tooltip.clone(), |checkbox, tooltip| {
                            checkbox.tooltip(move |window, cx| {
                                Tooltip::new(tooltip.clone()).build(window, cx)
                            })
                        })
                        .flex()
                        .items_center()
                        .gap_2()
                        .on_change(move |state, _, _, cx| {
                            let _ = owner.update(cx, |this, cx| {
                                this.emit_version(
                                    &id,
                                    revision,
                                    Action::Toggle(state == CheckboxState::Checked),
                                    cx,
                                )
                            });
                        })
                        .child(marker)
                        .child(label.clone()),
                    node.theme_role(),
                )
                .into_any_element()
            }
            Kind::Choice { options, selected } => {
                let mut group = RadioGroup::new(native_id.clone()).flex().flex_col().gap_2();
                for (index, option) in options.iter().enumerate() {
                    let owner = cx.entity().downgrade();
                    let checked = selected.as_ref() == Some(&option.id);
                    let node_id = node.id.clone();
                    let option_id = option.id.clone();
                    let marker = div()
                        .size(px(14.))
                        .rounded_full()
                        .border_1()
                        .border_color(colors.border)
                        .bg(if checked {
                            colors.accent
                        } else {
                            colors.background
                        });
                    group = group.child(
                        Radio::new(SharedString::from(format!("{}-option-{index}", node.id)))
                            .checked(checked)
                            .disabled(disabled || option.disabled)
                            .accessibility_label(option.label.clone())
                            .set_position(index + 1, options.len())
                            .flex()
                            .items_center()
                            .gap_2()
                            .on_change(move |_, _, _, cx| {
                                let _ = owner.update(cx, |this, cx| {
                                    this.emit(&node_id, Action::Select(option_id.clone()), cx)
                                });
                            })
                            .child(marker)
                            .child(option.label.clone()),
                    );
                }
                group.into_any_element()
            }
            Kind::List { items } => div()
                .flex()
                .flex_col()
                .gap_1()
                .children(items.iter().map(|text| div().py_1().child(text.clone())))
                .into_any_element(),
            Kind::Table { headers, rows } => {
                let mut table = div()
                    .flex()
                    .flex_col()
                    .border_1()
                    .border_color(colors.border);
                for (index, row) in std::iter::once(headers).chain(rows).enumerate() {
                    table = table.child(
                        div()
                            .flex()
                            .when(index == 0, |v| v.bg(colors.active))
                            .border_b_1()
                            .border_color(colors.border)
                            .children(
                                row.iter()
                                    .map(|cell| div().flex_1().min_w_0().p_2().child(cell.clone())),
                            ),
                    );
                }
                table.into_any_element()
            }
            Kind::Separator => div()
                .w_full()
                .h(px(1.))
                .bg(colors.border)
                .into_any_element(),
            Kind::Progress { label, value } => Progress::new(native_id.clone())
                .value(*value)
                .accessibility_label(label.clone())
                .h(px(8.))
                .w_full()
                .rounded(px(4.))
                .bg(colors.background)
                .border_1()
                .border_color(colors.border)
                .child(
                    div()
                        .h_full()
                        .w(relative(*value / 100.))
                        .bg(colors.accent)
                        .rounded(px(4.)),
                )
                .into_any_element(),
            Kind::Spacer => div().min_h(px(8.)).into_any_element(),
            _ => unreachable!("recursive geometry is dispatched before leaf controls"),
        }
    }
}
