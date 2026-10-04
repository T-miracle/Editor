//! Local A-layout presentation over Base button, popover-focus and positioning behavior.
use super::*;
use gpui_kit::AnyElement;

impl EditorApp {
    /// Pointer, touch and keyboard activation all enter the same captured viewing boundary.
    pub(crate) fn render_plugin_indicator(
        &self,
        kind: PluginPopupKind,
        cx: &Context<Self>,
    ) -> AnyElement {
        let count = self.plugin_count(kind, cx);
        let (id, label, icon) = match kind {
            PluginPopupKind::Loading => (
                "plugin-loading-indicator",
                t!("plugins.loading"),
                Spinner::new()
                    .icon(
                        Icon::default()
                            .data(include_bytes!("../../../assets/plugin-status/loading.svg")),
                    )
                    .small()
                    .into_any_element(),
            ),
            PluginPopupKind::Warning => (
                "plugin-warning-indicator",
                t!("plugins.warning"),
                StatusIcon::Warning.icon(cx).into_any_element(),
            ),
            PluginPopupKind::Error => (
                "plugin-error-indicator",
                t!("plugins.error"),
                StatusIcon::Error.icon(cx).into_any_element(),
            ),
        };
        let label = format!("{label} {count}");
        div()
            .debug_selector(move || id.into())
            .child(
                Button::new(id)
                    .ghost()
                    .small()
                    .h(px(22.))
                    .px_2()
                    .accessibility_label(label.clone())
                    .tooltip(label.clone())
                    .child(icon)
                    .child(label)
                    .on_click(cx.listener(move |this, event, window, cx| {
                        this.toggle_plugin_popup(kind, event, window, cx);
                    })),
            )
            .into_any_element()
    }

    /// The first outside press dismisses without activating editor chrome underneath the card.
    pub(crate) fn render_plugin_popup_blocker(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.plugin_popup.is_none() {
            return div().into_any_element();
        }
        div()
            .id("plugin-popup-blocker")
            .debug_selector(|| "plugin-popup-blocker".into())
            .absolute()
            .inset_0()
            .occlude()
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_up(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .on_any_mouse_down(cx.listener(|this, _, window, cx| {
                cx.stop_propagation();
                this.close_plugin_popup(window, cx);
            }))
            .into_any_element()
    }

    /// Keep the title fixed while grouped summaries scroll inside a window-bounded native card.
    pub(crate) fn render_plugin_popup(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some((_, position)) = self.plugin_popup else {
            // Existing grammar/LSP/worker completion clears the legacy tuple. Release Base state as well.
            if self.plugin_popup_snapshot.is_some() {
                self.close_plugin_popup(window, cx);
            }
            return div().into_any_element();
        };
        let mut loading = self.plugin_loading_groups(cx);
        let mut row_plugins: Vec<_> = self
            .plugin_popup_snapshot
            .as_ref()
            .into_iter()
            .flat_map(|snapshot| &snapshot.summaries)
            .map(|summary| Some(summary.record.plugin.clone()))
            .collect();
        if self
            .plugin_popup_snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.manager_error.is_some())
        {
            // The noninteractive error occupies a real scroll item before the focusable log rows.
            row_plugins.insert(0, None);
        }
        for (key, group) in &loading {
            if !row_plugins
                .iter()
                .any(|plugin| plugin.as_deref() == Some(key.as_str()))
            {
                row_plugins.push(group.plugin.clone());
            }
        }
        self.sync_plugin_popup_row_focus(&row_plugins, window, cx);
        let snapshot = self.plugin_popup_snapshot.as_ref();
        let mut rows = Vec::new();
        if let Some(snapshot) = snapshot {
            if let Some(error) = &snapshot.manager_error {
                // An ownerless recovery error has no routable plugin row but remains visible in the same Base popup.
                rows.push(
                    v_flex()
                        .debug_selector(|| "plugin-manager-status-detail".into())
                        .px_3()
                        .py_2()
                        .gap_1()
                        .child(
                            div()
                                .font_semibold()
                                .child(t!("plugins.manager_title").to_string()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .child(t!("plugins.manager_error", error = error).to_string()),
                        )
                        .into_any_element(),
                );
            }
            for summary in &snapshot.summaries {
                let loading = loading.remove(&summary.record.plugin);
                rows.push(self.render_plugin_summary(summary, loading.as_ref(), snapshot, cx));
            }
        }
        rows.extend(
            loading
                .into_values()
                .map(|loading| self.render_loading_summary(loading, cx)),
        );
        let width = px(440.).min((window.viewport_size().width - px(16.)).max(px(0.)));
        let height = px(420.).min((window.viewport_size().height - px(64.)).max(px(0.)));
        let mut body = v_flex()
            .id("plugin-status-scroll")
            .debug_selector(|| "plugin-status-scroll".into())
            .relative()
            .min_h_0()
            .max_h((height - px(44.)).max(px(0.)))
            .overflow_y_scroll()
            .children(rows);
        if let Some(snapshot) = snapshot {
            body =
                body.track_scroll(&snapshot.scroll)
                    .child(crate::ui::controls::vertical_scrollbar(
                        &snapshot.scroll,
                        cx,
                    ));
        }
        let mut card = v_flex()
            .id("plugin-status-popup")
            .debug_selector(|| "plugin-status-popup".into())
            .role(gpui_kit::Role::Dialog)
            .tab_group()
            .key_context("Popover")
            .w(width)
            .max_h(height)
            .overflow_hidden()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().popover)
            .text_color(cx.theme().foreground)
            .shadow_md()
            .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
            .on_action(
                cx.listener(|this, _: &gpui_base::actions::Cancel, window, cx| {
                    this.close_plugin_popup(window, cx);
                    cx.stop_propagation();
                }),
            )
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .font_semibold()
                            .child(t!("plugins.status_title").to_string()),
                    )
                    .child(
                        Button::new("plugin-status-close")
                            .ghost()
                            .small()
                            .compact()
                            .icon(IconName::Close)
                            .accessibility_label(t!("plugins.status_close").to_string())
                            .tooltip(t!("plugins.status_close").to_string())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_plugin_popup(window, cx)
                            })),
                    ),
            )
            .child(body);
        if let Some(snapshot) = snapshot {
            card = card.track_focus(&snapshot.state.read(cx).focus_handle(cx));
        }
        gpui_base::Positioner::side(Bounds::new(position, size(px(1.), px(1.))))
            .placement(gpui_base::Placement::Top)
            .align(gpui_base::Align::End)
            .offset(px(8.))
            .margin(px(8.))
            .occlude()
            .child(card)
            .into_any_element()
    }

    /// Observe Base focus without replacing its keyboard behavior; current row indices drive minimal scrolling.
    fn sync_plugin_popup_row_focus(
        &mut self,
        plugins: &[Option<String>],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let installed = &self.extensions.read(cx).entries;
        let rows: BTreeMap<_, _> = plugins
            .iter()
            .enumerate()
            .filter_map(|(index, plugin)| {
                plugin
                    .as_ref()
                    .filter(|plugin| {
                        installed
                            .iter()
                            .any(|entry| entry.manifest.id == plugin.as_str())
                    })
                    .map(|plugin| (plugin.clone(), index))
            })
            .collect();
        let Some(snapshot) = &mut self.plugin_popup_snapshot else {
            return;
        };
        // Completing or uninstalling a task drops its observer rather than accumulating per-frame listeners.
        snapshot
            .row_focus
            .retain(|plugin, _| rows.contains_key(plugin));
        for (plugin, index) in rows {
            if let Some(row) = snapshot.row_focus.get_mut(&plugin) {
                row.index = index;
                continue;
            }
            let handle = cx.focus_handle();
            let token = snapshot.token.clone();
            let watched = plugin.clone();
            let subscription = cx.on_focus_in(&handle, window, move |app, _, cx| {
                let Some(current) = app.plugin_popup_snapshot.as_ref().filter(|current| {
                    app.plugin_popup.is_some()
                        && Rc::ptr_eq(&current.token, &token)
                        && current.state.read(cx).is_open()
                }) else {
                    return;
                };
                if let Some(row) = current.row_focus.get(&watched) {
                    current.scroll.scroll_to_item(row.index);
                    cx.notify();
                }
            });
            snapshot.row_focus.insert(
                plugin,
                PluginPopupRowFocus {
                    handle,
                    index,
                    _subscription: subscription,
                },
            );
        }
    }

    /// The latest anomaly keeps its original severity; viewing is never represented as recovery.
    fn render_plugin_summary(
        &self,
        summary: &PluginSummary,
        loading: Option<&PluginLoadingSummary>,
        snapshot: &PluginPopupSnapshot,
        cx: &Context<Self>,
    ) -> AnyElement {
        let record = &summary.record;
        let record_id = record.id;
        let alert = severity_icon(Some(record.level)).expect("summary contains an anomaly");
        let source = record.source.replacen(':', " · ", 1);
        let content = v_flex()
            .debug_selector(move || format!("plugin-summary-record-{record_id}").into())
            .flex_shrink_0()
            .w_full()
            .min_w_0()
            // Rich summaries must override any inherited single-line button text geometry.
            .whitespace_normal()
            .line_height(gpui_kit::relative(1.4))
            .gap_2()
            .px_3()
            .py_3()
            .border_b_1()
            .border_color(cx.theme().border.opacity(0.65))
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .font_semibold()
                            .truncate()
                            .child(summary.name.clone()),
                    )
                    .child(
                        div()
                            .debug_selector(move || {
                                format!("plugin-summary-time-{record_id}").into()
                            })
                            .flex_shrink_0()
                            .whitespace_nowrap()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(log_time(record)),
                    ),
            )
            .child(
                h_flex().items_center().gap_1().child(alert.icon(cx)).child(
                    div()
                        .text_sm()
                        .font_semibold()
                        .child(level_label(record.level)),
                ),
            )
            .child(
                div()
                    .debug_selector(move || format!("plugin-summary-message-{record_id}").into())
                    .relative()
                    .w_full()
                    .min_w_0()
                    .whitespace_normal()
                    .text_sm()
                    .child(record.message.clone())
                    .child(self.read_visible_plugin_summary(record, snapshot, cx)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("plugins.status_source", source = source).to_string()),
            )
            .when_some(loading, |row, loading| {
                row.child(self.loading_details(&loading.details, cx))
            })
            .into_any_element();
        self.plugin_summary_entry(Some(&record.plugin), &summary.name, content, cx)
    }

    /// Normal history stays out of this card; only current work is merged beneath its plugin name.
    fn render_loading_summary(
        &self,
        loading: PluginLoadingSummary,
        cx: &Context<Self>,
    ) -> AnyElement {
        let content = v_flex()
            .flex_shrink_0()
            .w_full()
            .min_w_0()
            .whitespace_normal()
            .line_height(gpui_kit::relative(1.4))
            .gap_2()
            .px_3()
            .py_3()
            .border_b_1()
            .border_color(cx.theme().border.opacity(0.65))
            .child(div().font_semibold().child(loading.name.clone()))
            .child(self.loading_details(&loading.details, cx))
            .into_any_element();
        self.plugin_summary_entry(loading.plugin.as_deref(), &loading.name, content, cx)
    }

    /// Loading icons retain their native animation while detail text remains readable in either palette.
    fn loading_details(&self, details: &[String], cx: &Context<Self>) -> AnyElement {
        v_flex()
            .w_full()
            .min_w_0()
            .whitespace_normal()
            .gap_1()
            .child(
                h_flex()
                    .items_center()
                    .gap_1()
                    .child(Spinner::new().small())
                    .child(
                        div()
                            .text_sm()
                            .child(t!("plugins.status_loading_summary").to_string()),
                    ),
            )
            .children(details.iter().map(|detail| {
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(detail.clone())
            }))
            .into_any_element()
    }

    /// The entire card is one Base-backed target; unresolved/uninstalled history keeps full text contrast and no route.
    fn plugin_summary_entry(
        &self,
        plugin: Option<&str>,
        name: &str,
        content: AnyElement,
        cx: &Context<Self>,
    ) -> AnyElement {
        let Some(plugin) = plugin else {
            return content;
        };
        let installed = self
            .extensions
            .read(cx)
            .entries
            .iter()
            .any(|entry| entry.manifest.id == plugin);
        let plugin = plugin.to_owned();
        let selector = format!("plugin-summary-open-{plugin}");
        let wrapper = v_flex()
            .flex_shrink_0()
            .w_full()
            .debug_selector(move || selector.into());
        if !installed {
            return wrapper
                .child(content)
                .child(
                    div()
                        .px_3()
                        .pb_2()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(t!("plugins.status_uninstalled").to_string()),
                )
                .into_any_element();
        }
        wrapper
            .child(
                Button::new(format!("plugin-summary-action-{plugin}"))
                    .ghost()
                    .content_full_width()
                    .h_auto()
                    .w_full()
                    .min_w_0()
                    .justify_start()
                    .px_0()
                    .py_0()
                    .rounded_none()
                    .when_some(
                        self.plugin_popup_snapshot
                            .as_ref()
                            .and_then(|snapshot| snapshot.row_focus.get(&plugin)),
                        |button, row| button.track_focus(&row.handle),
                    )
                    .accessibility_label(format!("{} · {name}", t!("plugins.status_view_log")))
                    .child(content)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        // The manager route validates installation again in case it changed after this frame.
                        this.open_plugin_logs(plugin.clone(), window, cx);
                    })),
            )
            .into_any_element()
    }

    /// Reading requires the message to intersect the paint mask and still belong to this open snapshot.
    fn read_visible_plugin_summary(
        &self,
        record: &LogRecord,
        snapshot: &PluginPopupSnapshot,
        cx: &Context<Self>,
    ) -> AnyElement {
        let owner = cx.entity().downgrade();
        let plugin = record.plugin.clone();
        let record_id = record.id;
        let token = snapshot.token.clone();
        gpui_kit::canvas(
            |bounds, window, _| bounds.intersects(&window.content_mask().bounds),
            move |_, visible, _, cx| {
                if visible {
                    let owner = owner.clone();
                    let plugin = plugin.clone();
                    let token = token.clone();
                    cx.defer(move |cx| {
                        let _ = owner.update(cx, |app, cx| {
                            let current = app.plugin_popup.is_some()
                                && app.plugin_popup_snapshot.as_ref().is_some_and(|snapshot| {
                                    Rc::ptr_eq(&snapshot.token, &token)
                                        && snapshot.state.read(cx).is_open()
                                        && snapshot.summaries.iter().any(|summary| {
                                            summary.record.plugin == plugin
                                                && summary.record.id == record_id
                                        })
                                });
                            if current
                                && app
                                    .extensions
                                    .read(cx)
                                    .runtime_logs()
                                    .mark_read(&plugin, &[record_id])
                            {
                                cx.notify();
                            }
                        });
                    });
                }
            },
        )
        .absolute()
        .inset_0()
        .into_any_element()
    }
}
