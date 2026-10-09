//! Two native toolbar groups route captured targets; all plugin functions and artwork stay in packages.
use super::*;
use crate::outline::ToggleOutline;
use crate::ui::controls::menu::{MenuStyle, PopupMenu};

/// Native toolbar focus retains this context; reopening a file or replacing a window invalidates it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FunctionContext {
    File(protocol::api::FileVersion),
    Window { key: String, epoch: u64 },
}

#[derive(Clone)]
enum Action {
    Explorer,
    /// The built-in outline uses the same window controls and overflow behavior as other panels.
    Outline,
    Window {
        key: String,
        epoch: u64,
    },
    Function {
        key: String,
        epoch: u64,
        event: protocol::ui::ToolEvent,
    },
}

/// One immutable projection is shared by direct buttons and its group's overflow popup.
#[derive(Clone)]
struct Entry {
    id: String,
    label: String,
    tooltip: String,
    icon: Option<Icon>,
    selected: bool,
    disabled: bool,
    action: Action,
}

impl ExtensionPanel {
    /// Cache each resource once per package digest; repeated native rendering performs no disk reads.
    pub(super) fn refresh_tool_icons(&mut self) {
        let Some(key) = self
            .active
            .as_ref()
            .zip(self.surface_id.as_ref())
            .map(|(id, panel)| format!("{id}/{panel}"))
        else {
            return;
        };
        let Some(entry) = self
            .entries
            .iter()
            .find(|entry| Some(&entry.manifest.id) == self.active.as_ref())
        else {
            self.tool_icons.clear();
            return;
        };
        let wanted = self
            .views
            .get(&key)
            .into_iter()
            .flat_map(|doc| &doc.tools)
            .flat_map(|tool| [tool.icon.light.clone(), tool.icon.dark.clone()])
            .collect::<std::collections::BTreeSet<_>>();
        self.tool_icons.retain(|path, _| wanted.contains(path));
        for path in wanted {
            if !self.tool_icons.contains_key(&path) {
                let icon = entry
                    .tool_icon(&self.root, &path)
                    .map(|bytes| Icon::default().data(&bytes));
                self.tool_icons.insert(path, icon);
            }
        }
    }
}

impl EditorApp {
    /// Keep a target only while native focus is inside the toolbar or its captured overflow popup.
    fn update_function_context(&mut self, window: &Window, cx: &App) {
        let file = self
            .active_tab_index()
            .and_then(|index| self.plugin_file_context(index).ok())
            .map(|file| FunctionContext::File(file.version));
        for (key, panel) in &self.plugin_panels {
            let panel = panel.read(cx);
            if !panel.editor_preview
                && panel.visible.get()
                && (panel.focus.contains_focused(window, cx)
                    || panel
                        .native_ui
                        .as_ref()
                        .is_some_and(|view| view.read(cx).contains_focus(window, cx)))
            {
                self.function_context = Some(FunctionContext::Window {
                    key: key.clone(),
                    epoch: panel.instance_epoch,
                });
                return;
            }
        }
        if self.toolbar_focus.contains_focused(window, cx) || self.tool_overflow.is_some() {
            let valid = match &self.function_context {
                Some(FunctionContext::File(version)) => {
                    file.as_ref() == Some(&FunctionContext::File(version.clone()))
                }
                Some(FunctionContext::Window { key, epoch }) => {
                    self.plugin_panels.get(key).is_some_and(|panel| {
                        let panel = panel.read(cx);
                        !panel.editor_preview
                            && panel.visible.get()
                            && panel.instance_epoch == *epoch
                    })
                }
                None => false,
            };
            if valid {
                return;
            }
        }
        self.function_context = file;
    }

    /// Built-in panel toggles precede independent plugin windows in the window control group.
    fn window_entries(&self, explorer: Icon, cx: &App) -> Vec<Entry> {
        let mut windows = self
            .extensions
            .read(cx)
            .entries
            .iter()
            .filter(|entry| entry.enabled)
            .flat_map(|entry| {
                entry
                    .manifest
                    .panels
                    .iter()
                    .filter(|panel| panel.position != "editor")
                    .map(|panel| {
                        (
                            panel.status_order.unwrap_or(1000),
                            format!("{}/{}", entry.manifest.id, panel.id),
                        )
                    })
            })
            .collect::<Vec<_>>();
        windows.sort();
        let label = t!("panel.explorer").to_string();
        let mut result = vec![Entry {
            id: "explorer-panel-toggle".into(),
            label: label.clone(),
            tooltip: label,
            icon: Some(explorer),
            selected: self.explorer_visible,
            disabled: false,
            action: Action::Explorer,
        }];
        // Outline remains available when hidden, with selection reflecting its persisted visibility.
        result.push(Entry {
            id: "outline-toggle".into(),
            label: t!("panel.outline").to_string(),
            tooltip: t!(if self.session_state.outline_visible {
                "outline.hide"
            } else {
                "outline.show"
            })
            .to_string(),
            icon: Some(Icon::default().path("icons/outline-panel.svg")),
            selected: self.session_state.outline_visible,
            disabled: false,
            action: Action::Outline,
        });
        for (_, key) in windows {
            let Some(panel) = self.plugin_panels.get(&key) else {
                continue;
            };
            let panel = panel.read(cx);
            result.push(Entry {
                id: format!("plugin-window-{key}"),
                label: panel.panel_title.clone(),
                tooltip: panel.panel_title.clone(),
                icon: panel.panel_icon(cx.theme().is_dark()),
                selected: panel.visible.get(),
                disabled: false,
                action: Action::Window {
                    key,
                    epoch: panel.instance_epoch,
                },
            });
        }
        result
    }

    /// All eligible auxiliary and selected file surfaces contribute; window tools follow their own focus.
    fn function_entries(&self, cx: &App) -> Vec<Entry> {
        let mut tools = Vec::new();
        let locale = rust_i18n::locale();
        for (key, panel) in &self.plugin_panels {
            let panel = panel.read(cx);
            if !panel.visible.get() {
                continue;
            }
            let Some(document) = panel.current_document() else {
                continue;
            };
            for tool in &document.tools {
                let applies = match (&self.function_context, &tool.target) {
                    (
                        Some(FunctionContext::File(current)),
                        protocol::ui::ToolTarget::File { version },
                    ) => current == version,
                    (
                        Some(FunctionContext::Window {
                            key: current,
                            epoch,
                        }),
                        protocol::ui::ToolTarget::Window { panel: target },
                    ) => {
                        current == key
                            && *epoch == panel.instance_epoch
                            && panel.surface_id.as_ref() == Some(target)
                    }
                    _ => false,
                };
                if !applies || !tool.visible {
                    continue;
                }
                let Some(icon) = panel
                    .tool_icons
                    .get(tool.icon.path(cx.theme().is_dark()))
                    .and_then(Clone::clone)
                else {
                    continue;
                };
                tools.push((
                    tool.order,
                    key.clone(),
                    Entry {
                        id: format!("plugin-tool-{key}/{}", tool.id),
                        label: tool.label.for_locale(&locale).into(),
                        tooltip: tool.tooltip.for_locale(&locale).into(),
                        icon: Some(icon),
                        selected: tool.selected,
                        disabled: tool.disabled
                            || document.dialog.is_some()
                            || document.menu.is_some(),
                        action: Action::Function {
                            key: key.clone(),
                            epoch: panel.instance_epoch,
                            event: protocol::ui::ToolEvent {
                                revision: document.revision,
                                tool: tool.id.clone(),
                                target: tool.target.clone(),
                            },
                        },
                    },
                ));
            }
        }
        tools.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.id.cmp(&b.2.id)));
        tools.into_iter().map(|(_, _, entry)| entry).collect()
    }

    /// Revalidate native ownership after focus transfer; stale actions produce no plugin work.
    fn activate_bar_entry(&mut self, entry: &Entry, window: &mut Window, cx: &mut Context<Self>) {
        if entry.disabled {
            return;
        }
        match &entry.action {
            Action::Explorer => self.toggle_explorer(cx),
            Action::Outline => self.toggle_outline(&ToggleOutline, window, cx),
            Action::Window { key, epoch } => {
                let Some(panel) = self.plugin_panels.get(key).cloned() else {
                    return;
                };
                if panel.read(cx).instance_epoch != *epoch || panel.read(cx).editor_preview {
                    return;
                }
                panel.update(cx, |panel, cx| {
                    if panel.visible.get() {
                        panel.hide();
                    } else {
                        panel.show(window, cx);
                    }
                    cx.notify();
                });
                let visible = panel.read(cx).visible.get();
                self.function_context = visible.then(|| FunctionContext::Window {
                    key: key.clone(),
                    epoch: *epoch,
                });
                self.session_state
                    .plugin_panel_visibility
                    .insert(key.clone(), visible);
                self.persist_session();
                self.dock_area.update(cx, |_, cx| cx.notify());
            }
            Action::Function { key, epoch, event } => {
                let expected = match &event.target {
                    protocol::ui::ToolTarget::File { version } => {
                        Some(FunctionContext::File(version.clone()))
                    }
                    protocol::ui::ToolTarget::Window { .. } => Some(FunctionContext::Window {
                        key: key.clone(),
                        epoch: *epoch,
                    }),
                };
                if self.function_context != expected {
                    return;
                }
                if let protocol::ui::ToolTarget::File { version } = &event.target
                    && self
                        .active_tab_index()
                        .and_then(|index| self.plugin_file_context(index).ok())
                        .map(|file| file.version)
                        .as_ref()
                        != Some(version)
                {
                    return;
                }
                let Some(panel) = self.plugin_panels.get(key) else {
                    return;
                };
                let panel = panel.read(cx);
                if panel.instance_epoch != *epoch
                    || !panel.visible.get()
                    || panel
                        .current_document()
                        .is_none_or(|document| document.validate_tool_event(event).is_err())
                {
                    return;
                }
                panel.send(PluginEvent::Tool(event.clone()));
            }
        }
        cx.notify();
    }

    /// A group's hidden entries keep their state, target and native keyboard navigation in one popup.
    fn open_group_overflow(
        &mut self,
        entries: Vec<Entry>,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let items = entries
            .iter()
            .enumerate()
            .map(|(index, entry)| protocol::ui::MenuItem {
                id: index.to_string(),
                label: if entry.selected {
                    format!("✓ {}", entry.label)
                } else {
                    entry.label.clone()
                },
                disabled: entry.disabled,
                separator_before: false,
            })
            .collect();
        let parent = cx.entity().downgrade();
        self.tool_overflow = Some(cx.new(|cx| {
            PopupMenu::new(
                items,
                MenuStyle::current(cx),
                position,
                move |action, window, cx| {
                    let _ = parent.update(cx, |app, cx| {
                        if let protocol::ui::Action::Select(index) = action
                            && let Some(entry) = index
                                .parse::<usize>()
                                .ok()
                                .and_then(|index| entries.get(index))
                        {
                            app.activate_bar_entry(entry, window, cx);
                        }
                        app.tool_overflow = None;
                        cx.notify();
                    });
                },
                window,
                cx,
            )
        }));
        cx.notify();
    }

    /// Direct and overflow buttons share Base activation and the project's status-control appearance.
    fn render_bar_group(
        &self,
        entries: Vec<Entry>,
        slots: usize,
        group: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected_style = component_styles(cx, ThemeComponent::PanelToggle).selected;
        let visible = if entries.len() <= slots {
            entries.len()
        } else {
            slots.saturating_sub(1)
        };
        let mut row = h_flex()
            // Native verification and accessibility tooling can distinguish the two contribution groups.
            .debug_selector(move || format!("plugin-{group}-group").into())
            .tab_group()
            .items_center()
            .gap_1()
            .flex_shrink_0();
        for entry in entries.iter().take(visible) {
            let id = entry.id.clone();
            let button = Button::new(SharedString::from(id.clone()))
                .small()
                .compact()
                .ghost()
                .w(px(24.))
                .h(px(24.))
                .accessibility_label(entry.label.clone())
                .tooltip(entry.tooltip.clone())
                .disabled(entry.disabled)
                .when(entry.selected, |button| {
                    button
                        .bg(selected_style.background.unwrap_or(cx.theme().list_active))
                        .text_color(selected_style.foreground.unwrap_or(cx.theme().foreground))
                });
            let button = if let Some(icon) = &entry.icon {
                button.icon(icon.clone())
            } else {
                button.label("□")
            };
            let action = entry.clone();
            row = row.child(div().debug_selector(move || id.clone().into()).child(
                button.on_click(cx.listener(move |app, _, window, cx| {
                    app.activate_bar_entry(&action, window, cx)
                })),
            ));
        }
        if visible < entries.len() {
            let hidden = entries[visible..].to_vec();
            let id = format!("plugin-{group}-overflow");
            let label = if group == "windows" {
                t!("toolbar.more_windows")
            } else {
                t!("toolbar.more_tools")
            }
            .to_string();
            row = row.child(
                div().debug_selector(move || id.clone().into()).child(
                    Button::new(SharedString::from(format!("more-{group}")))
                        .label("⋯")
                        .small()
                        .compact()
                        .ghost()
                        .w(px(24.))
                        .h(px(24.))
                        .accessibility_label(label.clone())
                        .tooltip(label)
                        .on_click(cx.listener(
                            move |app, event: &gpui_kit::ClickEvent, window, cx| {
                                app.open_group_overflow(
                                    hidden.clone(),
                                    event.position(),
                                    window,
                                    cx,
                                )
                            },
                        )),
                ),
            );
        }
        // Keep the host message button inside the window group and outside plugin overflow,
        // so its red dot remains reachable even when the available width hides plugin windows.
        if group == "windows" {
            row = row.child(self.render_messages_button(cx));
        }
        row.into_any_element()
    }

    /// Reserve bounded single-line slots for each group; their overflow menus remain separate.
    pub(crate) fn render_plugin_toolbar(
        &mut self,
        explorer: Icon,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.update_function_context(window, cx);
        let windows = self.window_entries(explorer, cx);
        let functions = self.function_entries(cx);
        let slots = (((window.viewport_size().width / px(1.) - 220.).max(84.) / 28.).floor()
            as usize)
            .min(64)
            // Reserve a fixed slot for the host message window before dividing plugin slots.
            .saturating_sub(1);
        let window_slots = windows.len().min((slots / 2).max(1));
        let tool_slots = slots.saturating_sub(window_slots).max(1);
        let mut bar = h_flex()
            .items_center()
            .flex_shrink_0()
            .track_focus(&self.toolbar_focus)
            .child(self.render_bar_group(windows, window_slots, "windows", cx));
        if !functions.is_empty() {
            bar = bar
                .child(
                    div()
                        .debug_selector(|| "plugin-tool-group-separator".into())
                        .w(px(1.))
                        .h(px(14.))
                        .mx(px(4.))
                        .bg(cx.theme().border),
                )
                .child(self.render_bar_group(functions, tool_slots, "tools", cx));
        }
        bar.into_any_element()
    }
}
