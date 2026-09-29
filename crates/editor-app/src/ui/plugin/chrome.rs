//! Adapt portable canvas chrome to reusable native controls; no plugin identity is special-cased.
use crate::ui::controls::{
    menu::{MenuStyle, PopupMenu},
    side_tabs::{SideTabBar, SideTabsStyle},
};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;
use plugin_runtime::plugin_protocol::{
    Environment,
    ui::{CanvasChrome, UiEvent},
};
use std::{cell::Cell, rc::Rc};

pub(crate) struct ChromeView {
    plugin: String,
    model: CanvasChrome,
    environment: Environment,
    sidebar: Option<Entity<SideTabBar>>,
    menu: Option<Entity<PopupMenu>>,
    focus: FocusHandle,
    revision: Rc<Cell<u64>>,
    sink: Rc<dyn Fn(UiEvent, &mut App)>,
}
impl ChromeView {
    pub fn new(
        plugin: String,
        focus: FocusHandle,
        sink: impl Fn(UiEvent, &mut App) + 'static,
    ) -> Self {
        Self {
            plugin,
            focus,
            sink: Rc::new(sink),
            model: CanvasChrome::default(),
            environment: Environment::default(),
            sidebar: None,
            menu: None,
            revision: Rc::new(Cell::new(0)),
        }
    }
    fn color(&self, key: &str, fallback: Hsla) -> Hsla {
        self.environment
            .color(&self.plugin, &format!("ui.{key}"))
            .map(|v| rgb(v).into())
            .unwrap_or(fallback)
    }
    fn menu_style(&self, role: &str, cx: &App) -> MenuStyle {
        let mut style = MenuStyle::current(cx);
        style.surface = self.color(&format!("{role}.background"), style.surface);
        style.foreground = self.color(&format!("{role}.foreground"), style.foreground);
        style.border = self.color(&format!("{role}.border"), style.border);
        style.hover = self.color(&format!("{role}.hover_background"), style.hover);
        style.hover_foreground =
            self.color(&format!("{role}.hover_foreground"), style.hover_foreground);
        let font = self.environment.font_style(&self.plugin, role, false);
        if let Some(family) = font.family {
            style.font_family = family.into();
        }
        if let Some(size) = font.size_px {
            style.font_size = size;
        }
        style.bold = font.bold.unwrap_or(false);
        style
    }
    fn tabs_style(&self, cx: &App) -> SideTabsStyle {
        let mut menu = self.menu_style("tab", cx);
        menu.surface = self.color(
            "tab.inactive.background",
            self.color("tab_bar.background", cx.theme().tab_bar),
        );
        menu.foreground = self.color("tab.inactive.foreground", cx.theme().foreground);
        menu.border = self.color(
            "tab.border",
            self.color("tab_bar.border", cx.theme().border),
        );
        let color = |key: &str| {
            self.environment
                .color(&self.plugin, &format!("ui.{key}"))
                .map(|v| rgb(v).into())
        };
        SideTabsStyle {
            background: self.color("tab_bar.background", cx.theme().tab_bar),
            border: self.color("tab_bar.border", cx.theme().border),
            menu,
            active: self.color("tab.active.background", cx.theme().background),
            active_foreground: self.color("tab.active.foreground", cx.theme().foreground),
            close_background: color("tab.close.background"),
            close_foreground: color("tab.close.foreground"),
            rename_background: color("tab.rename.background"),
            rename_foreground: color("tab.rename.foreground"),
        }
    }
    pub fn update(
        &mut self,
        model: CanvasChrome,
        environment: Environment,
        origin: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.model == model && self.environment == environment {
            return;
        }
        let menu_changed =
            self.model.menu.as_ref().map(|m| &m.id) != model.menu.as_ref().map(|m| &m.id);
        let sidebar_changed =
            self.model.sidebar.as_ref().map(|m| &m.id) != model.sidebar.as_ref().map(|m| &m.id);
        self.environment = environment;
        self.revision.set(model.revision);
        if sidebar_changed {
            self.sidebar = None;
        }
        if let Some(tabs) = &model.sidebar {
            let style = self.tabs_style(cx);
            if self.sidebar.is_none() {
                let sink = self.sink.clone();
                let revision = self.revision.clone();
                let node = tabs.id.clone();
                let focus = self.focus.clone();
                self.sidebar = Some(cx.new(|cx| {
                    SideTabBar::new(
                        tabs.clone(),
                        style.clone(),
                        focus,
                        move |action, _, cx| {
                            sink(
                                UiEvent {
                                    revision: revision.get(),
                                    node: node.clone(),
                                    action,
                                },
                                cx,
                            )
                        },
                        cx,
                    )
                }));
            }
            self.sidebar
                .as_ref()
                .unwrap()
                .update(cx, |view, cx| view.update(tabs.clone(), style, window, cx));
        }
        if menu_changed {
            if model.menu.is_none() {
                self.focus.focus(window, cx);
            }
            self.menu = None;
        }
        if let Some(menu) = &model.menu {
            let style = self.menu_style("menu", cx);
            let position = origin + point(px(menu.x), px(menu.y));
            if let Some(view) = &self.menu {
                view.update(cx, |view, cx| {
                    view.items = menu.items.clone();
                    view.style = style;
                    view.position = position;
                    cx.notify();
                });
            } else {
                let sink = self.sink.clone();
                let revision = self.revision.clone();
                let node = menu.id.clone();
                self.menu = Some(cx.new(|cx| {
                    PopupMenu::new(
                        menu.items.clone(),
                        style,
                        position,
                        move |action, _, cx| {
                            sink(
                                UiEvent {
                                    revision: revision.get(),
                                    node: node.clone(),
                                    action,
                                },
                                cx,
                            )
                        },
                        window,
                        cx,
                    )
                }));
            }
        }
        self.model = model;
        cx.notify();
    }
}
impl Render for ChromeView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let mut view = div().size_full().relative();
        if let (Some(tabs), Some(sidebar)) = (&self.model.sidebar, &self.sidebar) {
            view = view.child(
                div()
                    .absolute()
                    .right_0()
                    .top_0()
                    .w(px(tabs.width))
                    .h_full()
                    .child(sidebar.clone()),
            );
        }
        if let Some(menu) = &self.menu {
            view = view.child(menu.clone());
        }
        view
    }
}

#[cfg(test)]
mod tests {
    use super::ChromeView;
    use gpui_kit::{TestAppContext, gpui, rgb};
    use plugin_runtime::plugin_protocol::FontStyle;
    #[gpui::test]
    fn chrome_theme_roles_follow_installed_theme_without_guest_paint(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::ui::typography::init(cx);
            crate::ui::theme::apply_theme(crate::ui::theme::builtin_theme(false), cx);
            let mut view = ChromeView::new("plugin".into(), cx.focus_handle(), |_, _| {});
            view.environment
                .theme_colors
                .insert("plugin.ui.menu.background".into(), 0x123456);
            view.environment
                .theme_colors
                .insert("plugin.ui.tab.active.foreground".into(), 0xabcdef);
            view.environment
                .theme_colors
                .insert("plugin.ui.tab.rename.background".into(), 0x345678);
            view.environment.theme_text_styles.insert(
                "plugin.tab".into(),
                FontStyle {
                    family: Some("Segoe UI".into()),
                    size_px: Some(19.),
                    bold: Some(true),
                },
            );
            assert_eq!(view.menu_style("menu", cx).surface, rgb(0x123456).into());
            let tabs = view.tabs_style(cx);
            assert_eq!(tabs.active_foreground, rgb(0xabcdef).into());
            assert_eq!(tabs.rename_background, Some(rgb(0x345678).into()));
            assert_eq!(tabs.menu.font_size, 19.);
            assert!(tabs.menu.bold);
            view.environment.theme_colors.clear();
            assert_ne!(view.menu_style("menu", cx).surface, rgb(0x123456).into());
        });
    }
}
