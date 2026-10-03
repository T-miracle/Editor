//! Adapt portable canvas controls to reusable native widgets; no plugin identity is special-cased.
use crate::ui::controls::{
    menu::{MenuStyle, PopupMenu},
    side_tabs::{SideTabBar, SideTabsStyle},
};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;
use plugin_runtime::plugin_protocol::{
    Environment,
    ui::{SideTabs, SideTabsPosition, UiEvent},
};
use std::{cell::Cell, rc::Rc};

/// Native reconciliation state only; the wire protocol exposes ordinary document nodes.
#[derive(Clone, Default, PartialEq)]
pub(super) struct CollectionModel {
    pub revision: u64,
    pub sidebar: Option<SideTabs>,
    pub menu: Option<plugin_runtime::plugin_protocol::ui::PopupMenu>,
}

pub(crate) struct CollectionView {
    plugin: String,
    model: CollectionModel,
    environment: Environment,
    sidebar: Option<Entity<SideTabBar>>,
    menu: Option<Entity<PopupMenu>>,
    focus: FocusHandle,
    revision: Rc<Cell<u64>>,
    sidebar_width: Rc<Cell<Option<f32>>>,
    sink: Rc<dyn Fn(UiEvent, &mut App)>,
}
impl CollectionView {
    pub fn new(
        plugin: String,
        focus: FocusHandle,
        sink: impl Fn(UiEvent, &mut App) + 'static,
    ) -> Self {
        Self {
            plugin,
            focus,
            sink: Rc::new(sink),
            model: CollectionModel::default(),
            environment: Environment::default(),
            sidebar: None,
            menu: None,
            revision: Rc::new(Cell::new(0)),
            sidebar_width: Rc::new(Cell::new(None)),
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
        // A neutral sidebar default remains overridable by the owning plugin's theme roles.
        let background = self.color("tab_bar.background", rgb(0xf7f8fa).into());
        // Document tabs use their selected border accent for both the label and selected edge.
        // Plugin-specific foreground and border roles remain independently overridable.
        let selected_border =
            crate::ui::theme::component_styles(cx, plugin_schema::ThemeComponent::EditorTab)
                .selected
                .border
                .unwrap_or(cx.theme().primary);
        let mut menu = self.menu_style("tab", cx);
        menu.surface = self.color("tab.inactive.background", background);
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
            background,
            border: self.color("tab_bar.border", cx.theme().border),
            menu,
            active: self.color("tab.active.background", cx.theme().background),
            active_foreground: self.color("tab.active.foreground", selected_border),
            active_inner_border: self.color("tab.active.inner_border", selected_border),
            close_background: color("tab.close.background"),
            close_foreground: color("tab.close.foreground"),
            rename_background: color("tab.rename.background"),
            rename_foreground: color("tab.rename.foreground"),
        }
    }
    pub fn update(
        &mut self,
        model: CollectionModel,
        environment: Environment,
        origin: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.model == model && self.environment == environment {
            return;
        }
        // Terminal output advances revisions without changing native controls; keep them stable.
        let controls_changed = self.model.sidebar != model.sidebar
            || self.model.menu != model.menu
            || self.environment != environment;
        if !controls_changed {
            self.revision.set(model.revision);
            self.model = model;
            return;
        }
        let menu_changed =
            self.model.menu.as_ref().map(|m| &m.id) != model.menu.as_ref().map(|m| &m.id);
        let sidebar_changed =
            self.model.sidebar.as_ref().map(|m| &m.id) != model.sidebar.as_ref().map(|m| &m.id);
        let width_changed = self.model.sidebar.as_ref().map(|tabs| tabs.width)
            != model.sidebar.as_ref().map(|tabs| tabs.width);
        let position_changed = self.model.sidebar.as_ref().map(|tabs| tabs.position)
            != model.sidebar.as_ref().map(|tabs| tabs.position);
        if sidebar_changed || width_changed || position_changed {
            // An updated guest width acknowledges a pending local divider preview.
            self.sidebar_width.set(None);
        }
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
                let preview_width = self.sidebar_width.clone();
                let owner = cx.entity().downgrade();
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
                        preview_width,
                        move |cx| {
                            let _ = owner.update(cx, |_, cx| cx.notify());
                        },
                        cx,
                    )
                }));
            }
            let mut displayed_tabs = tabs.clone();
            if let Some(width) = self.sidebar_width.get() {
                displayed_tabs.width = width;
            }
            self.sidebar.as_ref().unwrap().update(cx, |view, cx| {
                view.update(displayed_tabs, style, window, cx)
            });
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
impl Render for CollectionView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let mut view = div().size_full().relative();
        if let (Some(tabs), Some(sidebar)) = (&self.model.sidebar, &self.sidebar) {
            let dock = div()
                .absolute()
                .top_0()
                .w(px(self.sidebar_width.get().unwrap_or(tabs.width)))
                .h_full()
                .child(sidebar.clone());
            // Fixed width belongs to the declared dock edge, independent of panel resize.
            let dock = match tabs.position {
                SideTabsPosition::Left => dock.left_0(),
                SideTabsPosition::Right => dock.right_0(),
            };
            view = view.child(dock);
        }
        if let Some(menu) = &self.menu {
            view = view.child(menu.clone());
        }
        view
    }
}

#[cfg(test)]
mod tests {
    use super::{CollectionModel, CollectionView};
    use gpui_kit::{AppContext as _, TestAppContext, component::Root, gpui, point, px, rgb, size};
    use plugin_runtime::plugin_protocol::{
        Environment, FontStyle,
        ui::{SideTabs, SideTabsPosition},
    };
    use std::{cell::RefCell, rc::Rc};

    /// Output revisions must not roll back a native divider preview during a guest round trip.
    #[gpui::test]
    fn revision_only_update_preserves_sidebar_preview(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::ui::typography::init(cx);
            crate::ui::theme::apply_theme(crate::ui::theme::builtin_theme(false), cx);
        });
        let slot = Rc::new(RefCell::new(None));
        let capture = slot.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let view =
                cx.new(|cx| CollectionView::new("plugin".into(), cx.focus_handle(), |_, _| {}));
            *capture.borrow_mut() = Some(view.clone());
            Root::new(view, window, cx)
        });
        let view = slot.borrow_mut().take().unwrap();
        let tabs = SideTabs {
            id: "tabs".into(),
            position: SideTabsPosition::Right,
            items: Vec::new(),
            selected: None,
            rename: None,
            width: 180.,
            min_width: 80.,
            max_width: 480.,
        };
        let model = CollectionModel {
            revision: 1,
            sidebar: Some(tabs),
            menu: None,
        };
        cx.simulate_resize(size(px(600.), px(400.)));
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.update(
                    model.clone(),
                    Environment::default(),
                    point(px(0.), px(0.)),
                    window,
                    cx,
                )
            });
            let sidebar = view.read(cx).sidebar.as_ref().unwrap().clone();
            sidebar.update(cx, |bar, _| bar.model.width = 220.);
            view.read(cx).sidebar_width.set(Some(220.));
            let mut updated = model.clone();
            updated.revision = 2;
            view.update(cx, |view, cx| {
                view.update(
                    updated,
                    Environment::default(),
                    point(px(0.), px(0.)),
                    window,
                    cx,
                )
            });
            assert_eq!(sidebar.read(cx).model.width, 220.);
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert_eq!(
            cx.debug_bounds("native-side-tabs").unwrap().size.width,
            px(220.)
        );
        // Changing edges cancels a pending divider preview rather than reusing its old width.
        cx.update(|window, cx| {
            let mut updated = model;
            updated.revision = 3;
            updated.sidebar.as_mut().unwrap().position = SideTabsPosition::Left;
            view.update(cx, |view, cx| {
                view.update(
                    updated,
                    Environment::default(),
                    point(px(0.), px(0.)),
                    window,
                    cx,
                )
            });
            window.draw(cx).clear(cx);
        });
        let sidebar = cx.debug_bounds("native-side-tabs").unwrap();
        assert_eq!(sidebar.left(), px(0.));
        assert_eq!(sidebar.size.width, px(180.));
    }

    /// Each dock edge remains anchored without scaling the sidebar width during window resize.
    #[gpui::test]
    fn sidebar_width_stays_fixed_when_window_resizes(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::ui::typography::init(cx);
            crate::ui::theme::apply_theme(crate::ui::theme::builtin_theme(false), cx);
        });
        let slot = Rc::new(RefCell::new(None));
        let capture = slot.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let view =
                cx.new(|cx| CollectionView::new("plugin".into(), cx.focus_handle(), |_, _| {}));
            *capture.borrow_mut() = Some(view.clone());
            Root::new(view, window, cx)
        });
        let view = slot.borrow_mut().take().unwrap();
        let mut controls = CollectionModel {
            revision: 1,
            sidebar: Some(SideTabs {
                id: "tabs".into(),
                position: SideTabsPosition::Right,
                items: Vec::new(),
                selected: None,
                rename: None,
                width: 180.,
                min_width: 112.,
                max_width: 480.,
            }),
            menu: None,
        };
        for position in [SideTabsPosition::Right, SideTabsPosition::Left] {
            controls.sidebar.as_mut().unwrap().position = position;
            cx.update(|window, cx| {
                view.update(cx, |view, cx| {
                    view.update(
                        controls.clone(),
                        Environment::default(),
                        point(px(0.), px(0.)),
                        window,
                        cx,
                    )
                });
            });
            for width in [900., 320., 160., 800.] {
                cx.simulate_resize(size(px(width), px(400.)));
                cx.update(|window, cx| window.draw(cx).clear(cx));
                let sidebar = cx.debug_bounds("native-side-tabs").unwrap();
                assert_eq!(sidebar.size.width, px(180.));
                match position {
                    SideTabsPosition::Left => assert_eq!(sidebar.left(), px(0.)),
                    SideTabsPosition::Right => {
                        assert!((sidebar.right() - px(width)).abs() < px(1.))
                    }
                }
            }
        }
    }

    #[gpui::test]
    fn canvas_controls_theme_roles_follow_installed_theme_without_guest_paint(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::ui::typography::init(cx);
            crate::ui::theme::apply_theme(crate::ui::theme::builtin_theme(false), cx);
            let mut view = CollectionView::new("plugin".into(), cx.focus_handle(), |_, _| {});
            // Defaults are stable across host themes; explicit plugin roles take precedence.
            let tabs = view.tabs_style(cx);
            assert_eq!(tabs.background, rgb(0xf7f8fa).into());
            assert_eq!(tabs.menu.surface, tabs.background);
            assert_eq!(tabs.active_inner_border, rgb(0x3574f0).into());
            assert_eq!(tabs.active_foreground, rgb(0x3574f0).into());
            view.environment
                .theme_colors
                .insert("plugin.ui.menu.background".into(), 0x123456);
            view.environment
                .theme_colors
                .insert("plugin.ui.tab.active.foreground".into(), 0xabcdef);
            view.environment
                .theme_colors
                .insert("plugin.ui.tab.active.inner_border".into(), 0x456789);
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
            assert_eq!(tabs.active_inner_border, rgb(0x456789).into());
            assert_eq!(tabs.rename_background, Some(rgb(0x345678).into()));
            assert_eq!(tabs.menu.font_size, 19.);
            assert!(tabs.menu.bold);
            view.environment.theme_colors.clear();
            assert_ne!(view.menu_style("menu", cx).surface, rgb(0x123456).into());
            // Changing the editor's tab accent also changes an unoverridden plugin tab accent.
            let mut theme = crate::ui::theme::builtin_theme(false).clone();
            theme
                .components
                .get_mut(&plugin_schema::ThemeComponent::EditorTab)
                .unwrap()
                .selected
                .as_mut()
                .unwrap()
                .border = Some("#f13563".into());
            crate::ui::theme::apply_theme(&theme, cx);
            assert_eq!(
                view.tabs_style(cx).active_inner_border,
                rgb(0xf13563).into()
            );
            // Unoverridden labels follow the same editor accent when the theme changes.
            assert_eq!(view.tabs_style(cx).active_foreground, rgb(0xf13563).into());
            view.environment
                .theme_colors
                .insert("plugin.ui.tab.active.inner_border".into(), 0x456789);
            assert_eq!(
                view.tabs_style(cx).active_inner_border,
                rgb(0x456789).into()
            );
            assert_eq!(view.tabs_style(cx).active_foreground, rgb(0xf13563).into());
        });
    }
}
