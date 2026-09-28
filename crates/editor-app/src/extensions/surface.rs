//! Generic scene painting, native IME, shared scrollbars and package management controls.
use super::*;
use gpui_base::{Disableable, Scrollbar, ScrollbarHandle, ScrollbarMode};
use gpui_kit::component::IndexPath;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dialog::{DialogAction, DialogClose, DialogFooter};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::searchable_list::SearchableVec;
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::{
    ElementInputHandler, EntityInputHandler, FontWeight, TextRun, UTF16Selection, canvas, fill,
    font, rgb,
};
use std::{
    ops::Range,
    sync::mpsc,
    time::{Duration, Instant},
};

#[derive(Clone)]
pub(super) struct PluginScroll(Rc<RefCell<ScrollState>>);
struct ScrollState {
    bounds: Bounds<Pixels>,
    info: Option<protocol::ScrollInfo>,
    plugin: String,
    tx: mpsc::Sender<Work>,
    last_activity: Option<Instant>,
    dragging: bool,
    last_visible: bool,
}
impl PluginScroll {
    pub fn new(tx: mpsc::Sender<Work>) -> Self {
        Self(Rc::new(RefCell::new(ScrollState {
            bounds: Bounds::default(),
            info: None,
            plugin: String::new(),
            tx,
            last_activity: None,
            dragging: false,
            last_visible: false,
        })))
    }
    fn update(&self, bounds: Bounds<Pixels>, info: Option<protocol::ScrollInfo>, plugin: String) {
        let mut s = self.0.borrow_mut();
        // New history and changed offsets count as scrolling activity for declarative overlays.
        if matches!((&s.info, &info), (None, Some(_)))
            || matches!((&s.info, &info), (Some(old), Some(next)) if old.id != next.id || old.content != next.content || old.offset != next.offset)
            || s.plugin != plugin
        {
            s.last_activity = Some(Instant::now());
        }
        s.bounds = bounds;
        s.info = info;
        s.plugin = plugin;
    }
    fn note_activity(&self) {
        self.0.borrow_mut().last_activity = Some(Instant::now());
    }
    /// An optional guest timeout controls visibility without changing the scroll handle.
    fn visible_at(&self, info: &protocol::ScrollInfo, now: Instant) -> bool {
        let s = self.0.borrow();
        info.hide_after_ms.is_none_or(|ms| {
            s.dragging
                || s.last_activity.is_some_and(|activity| {
                    now.saturating_duration_since(activity) < Duration::from_millis(ms)
                })
        })
    }
    fn visible(&self, info: &protocol::ScrollInfo) -> bool {
        self.visible_at(info, Instant::now())
    }
    /// The existing UI poll wakes the panel once when an idle overlay expires.
    pub(super) fn visibility_changed(&self) -> bool {
        let current = {
            let s = self.0.borrow();
            s.info.as_ref().map(|info| info.clone())
        };
        let visible = current.as_ref().is_some_and(|info| self.visible(info));
        let mut s = self.0.borrow_mut();
        let changed = s.last_visible != visible;
        s.last_visible = visible;
        changed
    }
}
impl ScrollbarHandle for PluginScroll {
    fn viewport_bounds(&self) -> Bounds<Pixels> {
        self.0.borrow().bounds
    }
    fn offset(&self) -> Point<Pixels> {
        point(
            px(0.),
            px(-self
                .0
                .borrow()
                .info
                .as_ref()
                .map(|i| i.offset)
                .unwrap_or(0.)),
        )
    }
    fn content_size(&self) -> gpui_kit::Size<Pixels> {
        let s = self.0.borrow();
        size(
            s.bounds.size.width,
            px(s.info.as_ref().map(|i| i.content).unwrap_or(0.)),
        )
    }
    fn set_offset(&self, offset: Point<Pixels>) {
        let mut s = self.0.borrow_mut();
        s.last_activity = Some(Instant::now());
        if let Some(info) = &s.info {
            let _ = s.tx.send(Work::Event(
                s.plugin.clone(),
                PluginEvent::Scroll {
                    id: info.id.clone(),
                    offset: -offset.y / px(1.),
                },
            ));
        }
    }
    fn start_drag(&self) {
        let mut s = self.0.borrow_mut();
        s.dragging = true;
        s.last_activity = Some(Instant::now());
    }
    fn end_drag(&self) {
        let mut s = self.0.borrow_mut();
        s.dragging = false;
        s.last_activity = Some(Instant::now());
    }
}

#[cfg(test)]
mod scroll_tests {
    use super::*;

    /// An idle overlay disappears at the declared deadline and still forwards drag offsets.
    #[test]
    fn declared_scroll_timeout_preserves_native_scroll_events() {
        let (tx, rx) = mpsc::channel();
        let scroll = PluginScroll::new(tx);
        let info = protocol::ScrollInfo {
            id: "output".into(),
            rect: protocol::Rect::default(),
            content: 200.,
            offset: 0.,
            hide_after_ms: Some(1000),
        };
        scroll.update(Bounds::default(), Some(info.clone()), "me.terminal".into());
        let started = scroll.0.borrow().last_activity.unwrap();
        assert!(scroll.visible_at(&info, started + Duration::from_millis(999)));
        assert!(!scroll.visible_at(&info, started + Duration::from_millis(1000)));
        assert!(
            scroll.visibility_changed(),
            "the panel must paint the newly visible bar"
        );
        scroll.0.borrow_mut().last_activity = Some(started - Duration::from_millis(1000));
        assert!(
            scroll.visibility_changed(),
            "the idle UI poll must repaint to hide it"
        );
        scroll.set_offset(point(px(0.), px(-40.)));
        assert!(
            matches!(rx.try_recv(), Ok(Work::Event(id, PluginEvent::Scroll { offset, .. })) if id == "me.terminal" && offset == 40.)
        );
        let refreshed = scroll.0.borrow().last_activity.unwrap();
        assert!(scroll.visible_at(&info, refreshed + Duration::from_millis(999)));
        scroll.start_drag();
        scroll.0.borrow_mut().last_activity = Some(refreshed - Duration::from_millis(1000));
        assert!(
            scroll.visible(&info),
            "the thumb must remain available while dragging"
        );
        scroll.end_drag();
    }
}
impl ExtensionPanel {
    /// Paint generic rectangles and text with the native text shaper; no terminal types are linked.
    fn paint(&mut self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(scene) = self.current_scene() else {
            return;
        };
        if self.focus.is_focused(window) {
            window.handle_input(
                &self.focus,
                ElementInputHandler::new(bounds, cx.entity()),
                cx,
            );
        }
        for operation in &scene.paint {
            match operation {
                protocol::Paint::Fill {
                    rect,
                    color,
                    extend_to_bottom,
                } => {
                    // Dock drags resize the native canvas before the guest handles Resize.
                    let rect = if *extend_to_bottom {
                        protocol::Rect {
                            h: (bounds.size.height / px(1.) - rect.y).max(0.),
                            ..*rect
                        }
                    } else {
                        *rect
                    };
                    window.paint_quad(fill(rect_bounds(rect, bounds.origin), rgb(*color)));
                }
                protocol::Paint::Text {
                    x,
                    y,
                    text,
                    color,
                    size: font_size,
                    bold,
                } => {
                    let mut face = font(scene.font.clone());
                    if *bold {
                        face.weight = FontWeight::BOLD;
                    }
                    let run = TextRun {
                        len: text.len(),
                        font: face,
                        color: rgb(*color).into(),
                        ..Default::default()
                    };
                    let line = window.text_system().shape_line(
                        text.clone().into(),
                        px(*font_size),
                        &[run],
                        None,
                    );
                    let _ = line.paint(
                        bounds.origin + point(px(*x), px(*y)),
                        px((font_size * 1.45).ceil()),
                        gpui_kit::TextAlign::Left,
                        None,
                        window,
                        cx,
                    );
                }
            }
        }
        if !self.composition.is_empty() {
            let run = TextRun {
                len: self.composition.len(),
                font: font(scene.font.clone()),
                color: cx.theme().foreground,
                background_color: Some(cx.theme().background),
                ..Default::default()
            };
            let line = window.text_system().shape_line(
                self.composition.clone().into(),
                px(scene.font_size),
                &[run],
                None,
            );
            let _ = line.paint(
                bounds.origin + point(px(scene.cursor.x), px(scene.cursor.y)),
                px(scene.cursor.h),
                gpui_kit::TextAlign::Left,
                None,
                window,
                cx,
            );
        }
    }
    fn pointer(&self, kind: &str, position: Point<Pixels>, button: u8, clicks: u8, shift: bool) {
        self.send(PluginEvent::Pointer {
            kind: kind.into(),
            x: (position.x - self.bounds.left()) / px(1.),
            y: (position.y - self.bounds.top()) / px(1.),
            button,
            clicks,
            shift,
        });
    }
    /// Show package origin and requested capabilities above the manager's README.
    fn open_install_dialog(
        &mut self,
        package: Package,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let executable = package.manifest.component.is_some();
        let name = package.manifest.name.clone();
        let version = package.manifest.version.clone();
        let source = package.source.clone();
        let permissions = package.manifest.permissions.clone();
        let owner = cx.entity().downgrade();
        let install_owner = owner.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let name = name.clone();
            let source = source.clone();
            let permissions = permissions.clone();
            let install_package = package.clone();
            let install_owner = install_owner.clone();
            let cancel_owner = owner.clone();
            dialog
                .title(format!("安装插件 · {name} v{version}"))
                .width(px(520.))
                .overlay_closable(false)
                .close_button(false)
                .content(move |content, _, cx| {
                    let mut details = v_flex()
                        .id("plugin-install-consent")
                        .debug_selector(|| "plugin-install-consent".into())
                        .gap_3()
                        .child(
                            div()
                                .text_color(cx.theme().muted_foreground)
                                .child("本机未签名插件，请确认安装来源和所需能力。"),
                        );
                    if let Some(source) = &source {
                        details = details.child(format!("来源：{source}"));
                    }
                    details = details.child(if executable {
                        "安装后将允许以下能力："
                    } else {
                        "此插件仅提供声明式资源，无需额外运行权限。"
                    });
                    for permission in &permissions {
                        let explanation = match permission.as_str() {
                            "process.pty" => {
                                "启动本机程序：这些程序以当前用户权限运行，可访问本机文件与网络"
                            }
                            "workspace.read" => "读取当前工作区文件",
                            "clipboard" => "读写系统剪贴板",
                            "storage" => "保存插件私有配置与会话数据",
                            "editor.commands" => "读取选区、保存文件和打开插件配置",
                            _ => permission,
                        };
                        details = details.child(format!("• {explanation}"));
                    }
                    details = details.child(if executable {
                        "更新将停止插件当前运行的程序，保存会话后启动新程序。原有命令不会自动重跑。"
                    } else {
                        "更新后会重新加载语法、主题或图标资源。"
                    });
                    content.child(details)
                })
                // Base Dialog needs an explicit footer; button props only label actions.
                .footer(
                    DialogFooter::new()
                        .child(
                            div()
                                .id("plugin-install-cancel")
                                .debug_selector(|| "plugin-install-cancel".into())
                                .child(DialogClose::new().trigger(|button| button.label("取消"))),
                        )
                        .child(
                            div()
                                .id("plugin-install-confirm")
                                .debug_selector(|| "plugin-install-confirm".into())
                                .child(
                                    DialogAction::new().child(
                                        Button::new("confirm-plugin-install")
                                            .label("确认安装")
                                            .primary(),
                                    ),
                                ),
                        ),
                )
                .on_ok(move |_, _, cx| {
                    install_owner
                        .update(cx, |this, cx| {
                            if this.queue_lifecycle(Work::Install(install_package.clone())) {
                                this.pending = None;
                                this.pending_dialog_open = false;
                                cx.notify();
                                true
                            } else {
                                false
                            }
                        })
                        .unwrap_or(false)
                })
                .on_cancel(move |_, _, cx| {
                    let _ = cancel_owner.update(cx, |this, cx| {
                        this.pending = None;
                        this.pending_dialog_open = false;
                        cx.notify();
                    });
                    true
                })
        });
    }

    /// Install and destructive lifecycle actions have explicit, reviewable native controls.
    fn manager(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if self.manager_search.is_none() {
            // Keep search input state alive across manager repaints and focus changes.
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("搜索插件"));
            self.manager_search_subscription =
                Some(cx.subscribe(&input, |this, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.manager_selected = None;
                        cx.notify();
                    }
                }));
            self.manager_search = Some(input);
        }
        let busy = self.progress.is_some();
        if let Some(package) = self.pending.clone().filter(|_| !self.pending_dialog_open) {
            // Defer the overlay until the manager render finishes updating its Root.
            self.pending_dialog_open = true;
            cx.defer_in(window, move |this, window, cx| {
                if this
                    .pending
                    .as_ref()
                    .is_some_and(|pending| pending.digest == package.digest)
                {
                    this.open_install_dialog(package, window, cx);
                }
            });
        }
        let query = self
            .manager_search
            .as_ref()
            .map(|input| input.read(cx).value().to_string())
            .unwrap_or_default()
            .to_lowercase();
        let installed = self.entries.iter().filter(|entry| {
            !self.manager_market
                && (entry.manifest.name.to_lowercase().contains(&query)
                    || entry.manifest.id.to_lowercase().contains(&query))
        });
        let market = self.manager_packages.iter().filter(|package| {
            self.manager_market
                && (package.manifest.name.to_lowercase().contains(&query)
                    || package.manifest.id.to_lowercase().contains(&query))
        });
        // One list drives both tabs; each row keeps its stable package identity.
        let choices: Vec<(String, String, String)> = installed
            .map(|entry| {
                (
                    entry.manifest.id.clone(),
                    entry.manifest.name.clone(),
                    entry.manifest.version.clone(),
                )
            })
            .chain(market.map(|package| {
                (
                    package.manifest.id.clone(),
                    package.manifest.name.clone(),
                    package.manifest.version.clone(),
                )
            }))
            .collect();
        let selected_id = self
            .manager_selected
            .as_ref()
            .filter(|id| choices.iter().any(|choice| &choice.0 == *id))
            .cloned()
            .or_else(|| choices.first().map(|choice| choice.0.clone()));
        let selected_entry = selected_id
            .as_ref()
            .and_then(|id| self.entries.iter().find(|entry| &entry.manifest.id == id));
        let selected_package = selected_id.as_ref().and_then(|id| {
            self.manager_packages
                .iter()
                .find(|package| &package.manifest.id == id)
        });
        let selected_manifest = if self.manager_market {
            selected_package.map(|package| &package.manifest)
        } else {
            selected_entry.map(|entry| &entry.manifest)
        };
        let selected_global =
            selected_entry.map(|entry| entry.global_enabled.unwrap_or(entry.enabled));
        if self.manager_scope_for != selected_id || self.manager_scope_global != selected_global {
            // The native select owns focus and its selected value for one detail target.
            self.manager_scope = selected_global.map(|enabled| {
                cx.new(|cx| {
                    SelectState::new(
                        SearchableVec::new(vec!["默认全局启动", "默认全局禁用"]),
                        Some(IndexPath::new(usize::from(!enabled))),
                        window,
                        cx,
                    )
                })
            });
            self.manager_scope_subscription = self.manager_scope.as_ref().map(|scope| {
                cx.subscribe(
                    scope,
                    |this, _, event: &SelectEvent<SearchableVec<&'static str>>, cx| {
                        let SelectEvent::Confirm(Some(value)) = event else {
                            return;
                        };
                        let Some(id) = this.manager_scope_for.clone() else {
                            return;
                        };
                        // Selection applies immediately; it does not reuse the uninstall confirmation.
                        let enable = match *value {
                            "默认全局启动" => true,
                            "默认全局禁用" => false,
                            _ => return,
                        };
                        if this.manager_scope_global == Some(enable) {
                            return;
                        }
                        this.manager_scope_global = None;
                        this.queue_lifecycle(if enable {
                            Work::Enable(id)
                        } else {
                            Work::Disable(id)
                        });
                        cx.notify();
                    },
                )
            });
            self.manager_scope_for = selected_id.clone();
            self.manager_scope_global = selected_global;
        }
        let mut list = v_flex().gap_1();
        for (id, name, version) in &choices {
            let row_id = id.clone();
            let selected = selected_id.as_ref() == Some(id);
            list = list.child(
                h_flex()
                    .id(SharedString::from(format!("plugin-row-{id}")))
                    .w_full()
                    .px_3()
                    .py_2()
                    .gap_2()
                    .items_center()
                    .cursor_pointer()
                    .when(selected, |row| row.bg(cx.theme().list_active))
                    .when(!selected, |row| {
                        row.hover(|style| style.bg(cx.theme().list_hover))
                    })
                    .child(div().flex_1().min_w(px(0.)).truncate().child(name.clone()))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(version.clone()),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.manager_selected = Some(row_id.clone());
                        this.pending = None;
                        this.confirm = None;
                        cx.notify();
                    })),
            );
        }
        let sidebar = v_flex()
            .id("plugin-manager-sidebar")
            .debug_selector(|| "plugin-manager-sidebar".into())
            .w(px(258.))
            .h_full()
            .flex_shrink_0()
            .border_r_1()
            .border_color(cx.theme().sidebar_border)
            .bg(cx.theme().sidebar)
            .child(
                v_flex()
                    .p_3()
                    .gap_3()
                    .child(Input::new(self.manager_search.as_ref().unwrap()))
                    .child(
                        div()
                            .id("plugin-manager-tab-strip")
                            .debug_selector(|| "plugin-manager-tab-strip".into())
                            .child(
                                TabBar::new("plugin-manager-tabs")
                                    .underline()
                                    .selected_index(usize::from(self.manager_market))
                                    .child(Tab::new().label("已安装"))
                                    .child(Tab::new().label("插件市场"))
                                    .on_click({
                                        let owner = cx.entity().downgrade();
                                        move |index, _, cx| {
                                            let _ = owner.update(cx, |this, cx| {
                                                let market = *index == 1;
                                                if market && !this.manager_market {
                                                    this.load_market_packages();
                                                }
                                                this.manager_market = market;
                                                this.manager_selected = None;
                                                cx.notify();
                                            });
                                        }
                                    }),
                            ),
                    ),
            )
            .child(div().flex_1().overflow_y_scrollbar().child(list))
            .child(
                div()
                    .p_3()
                    .border_t_1()
                    .border_color(cx.theme().sidebar_border)
                    .child(
                        Button::new("install-local-plugin")
                            .label("从本机安装 ZIP…")
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| this.choose_package(cx))),
                    ),
            );
        let mut detail = v_flex()
            .id("plugin-manager-detail")
            .debug_selector(|| "plugin-manager-detail".into())
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .overflow_y_scrollbar()
            .bg(cx.theme().background);
        if let Some(manifest) = selected_manifest {
            let id = manifest.id.clone();
            let installed_version = selected_entry.map(|entry| entry.manifest.version.as_str());
            let action = if !self.manager_market {
                "卸载"
            } else if let Some(current) = installed_version {
                match (
                    semver::Version::parse(&manifest.version),
                    semver::Version::parse(current),
                ) {
                    (Ok(available), Ok(installed)) if available > installed => "更新",
                    (Ok(available), Ok(installed)) if available < installed => "降级安装",
                    _ => "已安装",
                }
            } else {
                "安装"
            };
            let package_path = self
                .manager_market
                .then_some(selected_package)
                .flatten()
                .and_then(|package| package.source.as_ref())
                .map(PathBuf::from);
            let global_enabled =
                selected_entry.is_some_and(|entry| entry.global_enabled.unwrap_or(entry.enabled));
            let project_enabled = selected_entry.is_some_and(|entry| {
                entry
                    .project_enabled
                    .contains(&self.workspace.to_string_lossy().to_string())
            });
            let action_id = id.clone();
            // Both ZIP inspection and installation keep the selected action visibly busy.
            let primary_loading = self.progress.as_ref().is_some_and(|progress| {
                progress.action == LifecycleAction::Inspect
                    || (progress.action == LifecycleAction::Install && progress.id == id)
            });
            let project_id = id.clone();
            let project_owner = cx.entity().downgrade();
            detail = detail.child(
                v_flex()
                    .p_5()
                    .gap_4()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        h_flex()
                            .gap_3()
                            .items_center()
                            .child(div().text_xl().font_semibold().child(manifest.name.clone()))
                            .child(
                                div()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("v{}", manifest.version)),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .id("plugin-primary-action-region")
                                    .debug_selector(|| "plugin-primary-action-region".into())
                                    .child(
                                        Button::new("plugin-primary-action")
                                            .label(action)
                                            .when(action != "已安装", |button| button.primary())
                                            .when(primary_loading, |button| {
                                                button.icon(IconName::Loader)
                                            })
                                            .loading(primary_loading)
                                            .disabled(busy || action == "已安装")
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if let Some(path) = package_path.clone() {
                                                    this.queue_lifecycle(Work::Inspect(path));
                                                } else {
                                                    this.confirm = Some((action_id.clone(), true));
                                                }
                                                cx.notify();
                                            })),
                                    ),
                            )
                            .when(selected_entry.is_some(), |row| {
                                row.child(
                                    Select::new(self.manager_scope.as_ref().unwrap())
                                        .id("plugin-global-scope")
                                        .disabled(busy)
                                        .w(px(176.)),
                                )
                            })
                            .when(selected_entry.is_some() && !global_enabled, |row| {
                                row.child(
                                    Checkbox::new("plugin-project-enabled")
                                        .label("本项目启用")
                                        .checked(project_enabled)
                                        .disabled(busy)
                                        .on_change(move |checked, _, cx| {
                                            let checked = *checked;
                                            let _ = project_owner.update(cx, |this, cx| {
                                                let _ =
                                                    this.worker.tx.send(Work::SetProjectEnabled(
                                                        project_id.clone(),
                                                        checked,
                                                    ));
                                                cx.notify();
                                            });
                                        }),
                                )
                            }),
                    ),
            );
            if self.confirm.is_none() && self.status.is_none() {
                let readme = self
                    .manager_market
                    .then_some(selected_package)
                    .flatten()
                    .and_then(|package| package.files.get("README.md"))
                    .and_then(|bytes| std::str::from_utf8(bytes).ok())
                    .map(str::to_owned)
                    .or_else(|| {
                        selected_entry.and_then(|entry| {
                            std::fs::read_to_string(
                                self.root
                                    .join("packages")
                                    .join(&entry.manifest.id)
                                    .join(&entry.digest)
                                    .join("README.md"),
                            )
                            .ok()
                        })
                    });
                // README content uses the same Markdown renderer as editor popovers.
                detail = detail.child(
                    div()
                        .id("plugin-readme-region")
                        .debug_selector(|| "plugin-readme-region".into())
                        .p_5()
                        .child(
                            gpui_kit::component::text::TextView::markdown(
                                "plugin-readme",
                                readme.unwrap_or_else(|| "此插件没有提供 README.md。".into()),
                            )
                            .selectable(true),
                        ),
                );
            }
            if let Some(error) = selected_entry.and_then(|entry| entry.error.as_ref()) {
                detail = detail.child(
                    div()
                        .px_5()
                        .text_color(cx.theme().danger)
                        .child(error.clone()),
                );
            }
        } else {
            detail = detail.child(div().p_5().text_color(cx.theme().muted_foreground).child(
                if self.manager_market {
                    "插件市场暂无可用插件包。"
                } else {
                    "暂无已安装插件。"
                },
            ));
        }
        let mut content = detail;
        if let Some((id, uninstall)) = &self.confirm {
            let id = id.clone();
            let uninstalling = self.progress.as_ref().is_some_and(|progress| {
                progress.action == LifecycleAction::Uninstall && progress.id == id
            });
            let remove = *uninstall;
            let preserve_id = id.clone();
            let delete_id = id.clone();
            let count = self.processes.get(&id).copied().unwrap_or(0);
            let executable = self
                .entries
                .iter()
                .find(|entry| entry.manifest.id == id)
                .is_some_and(|entry| entry.manifest.component.is_some());
            // The package ID stays stable internally, while confirmations use its display name.
            let name = self
                .entries
                .iter()
                .find(|entry| entry.manifest.id == id)
                .map(|entry| entry.manifest.name.as_str())
                .unwrap_or_else(|| id.strip_prefix("me.").unwrap_or(&id));
            content = content
                .child(if executable {
                    format!(
                        "{} {}：将关闭 {count} 个运行中的程序。",
                        if remove { "卸载" } else { "停用" },
                        name
                    )
                } else {
                    format!(
                        "{} {}：将撤销其声明式资源。",
                        if remove { "卸载" } else { "停用" },
                        name
                    )
                })
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("confirm-plugin-preserve")
                                .label(if remove {
                                    "卸载，保留数据"
                                } else {
                                    "确认停用"
                                })
                                .when(
                                    uninstalling
                                        && self
                                            .progress
                                            .as_ref()
                                            .is_some_and(|p| p.delete_data == Some(false)),
                                    |button| button.icon(IconName::Loader),
                                )
                                .loading(
                                    uninstalling
                                        && self
                                            .progress
                                            .as_ref()
                                            .is_some_and(|p| p.delete_data == Some(false)),
                                )
                                .disabled(busy)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let queued = if remove {
                                        this.queue_lifecycle(Work::Uninstall(
                                            preserve_id.clone(),
                                            false,
                                        ))
                                    } else {
                                        this.worker
                                            .tx
                                            .send(Work::Disable(preserve_id.clone()))
                                            .is_ok()
                                    };
                                    if queued {
                                        if !remove {
                                            this.confirm = None;
                                        }
                                    }
                                    cx.notify();
                                })),
                        )
                        .when(remove, |row| {
                            row.child(
                                Button::new("confirm-plugin-delete")
                                    .label("卸载并删除数据")
                                    .when(
                                        uninstalling
                                            && self
                                                .progress
                                                .as_ref()
                                                .is_some_and(|p| p.delete_data == Some(true)),
                                        |button| button.icon(IconName::Loader),
                                    )
                                    .loading(
                                        uninstalling
                                            && self
                                                .progress
                                                .as_ref()
                                                .is_some_and(|p| p.delete_data == Some(true)),
                                    )
                                    .disabled(busy)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.queue_lifecycle(Work::Uninstall(
                                            delete_id.clone(),
                                            true,
                                        ));
                                        cx.notify();
                                    })),
                            )
                        })
                        .child(
                            Button::new("cancel-plugin-remove")
                                .label("取消")
                                .disabled(busy)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.confirm = None;
                                    cx.notify();
                                })),
                        ),
                );
        }
        if let Some(status) = &self.status {
            content = content.child(div().text_color(cx.theme().danger).child(status.clone()));
        }
        h_flex()
            .id("runtime-plugin-manager")
            .debug_selector(|| "runtime-plugin-manager".into())
            .size_full()
            .items_start()
            .child(sidebar)
            .child(content)
            .into_any_element()
    }
}
fn rect_bounds(rect: protocol::Rect, origin: Point<Pixels>) -> Bounds<Pixels> {
    Bounds::new(
        origin + point(px(rect.x), px(rect.y)),
        size(px(rect.w), px(rect.h)),
    )
}
impl Render for ExtensionPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.manager_open || self.surface_id.is_none() {
            return self.manager(window, cx);
        }
        if self._focus_events.is_empty() {
            self._focus_events
                .push(cx.on_focus(&self.focus, window, |this, _, _| {
                    this.send(PluginEvent::Focus(true))
                }));
            self._focus_events
                .push(cx.on_blur(&self.focus, window, |this, _, cx| {
                    this.composition.clear();
                    this.send(PluginEvent::Focus(false));
                    cx.notify();
                }));
        }
        self.sync_edit(window, cx);
        let environment = environment(&self.workspace, cx);
        // Plugin-only theme tokens trigger the same live update as editor base colors.
        if self.last_theme.as_ref() != Some(&environment) {
            self.last_theme = Some(environment.clone());
            for entry in &self.entries {
                if entry.enabled && Some(&entry.manifest.id) == self.active.as_ref() {
                    self.send_to(&entry.manifest.id, PluginEvent::Theme(environment.clone()));
                }
            }
        }
        let prepaint = cx.entity().downgrade();
        let paint = prepaint.clone();
        let scene = self.current_scene();
        let mut view = div()
            .id("plugin-surface")
            .debug_selector(|| "plugin-surface".into())
            .key_context("PluginSurface")
            .size_full()
            .relative()
            .overflow_hidden()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if this.editing.is_some() || !this.composition.is_empty() {
                    return;
                }
                if this.shortcut(event, window, cx) {
                    return;
                }
                let key = &event.keystroke;
                this.send(PluginEvent::Key {
                    key: key.key.clone(),
                    ctrl: key.modifiers.control,
                    alt: key.modifiers.alt,
                    shift: key.modifiers.shift,
                });
                // GPUI names the printable space key "space"; let native text/IME commit it.
                if key.modifiers.control
                    || key.modifiers.alt
                    || (key.key.len() > 1 && key.key != "space")
                {
                    cx.stop_propagation();
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    this.focus(window, cx);
                    this.pointer(
                        "down",
                        event.position,
                        0,
                        event.click_count as u8,
                        event.modifiers.shift,
                    );
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, _, _| {
                    this.pointer("up", event.position, 0, 1, event.modifiers.shift)
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    this.focus(window, cx);
                    this.pointer(
                        "down",
                        event.position,
                        2,
                        event.click_count as u8,
                        event.modifiers.shift,
                    );
                    cx.stop_propagation();
                }),
            )
            .on_mouse_up(
                MouseButton::Right,
                cx.listener(|this, event: &MouseUpEvent, _, _| {
                    this.pointer("up", event.position, 2, 1, event.modifiers.shift)
                }),
            )
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(|this, event: &MouseDownEvent, _, _| {
                    this.pointer("down", event.position, 1, 1, event.modifiers.shift)
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &gpui_kit::MouseMoveEvent, _, _| {
                if event.pressed_button.is_some() {
                    this.pointer("move", event.position, 0, 1, event.modifiers.shift);
                }
            }))
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                let delta = event.delta.pixel_delta(px(this.last_size.3.max(1.))).y
                    / px(this.last_size.3.max(1.));
                let position = event.position - this.bounds.origin;
                // Revealing the overlay is generic UI behavior; the plugin chooses its timeout.
                if delta != 0.
                    && this
                        .current_scene()
                        .and_then(|scene| scene.scroll.clone())
                        .is_some_and(|scroll| {
                            scroll
                                .rect
                                .contains(position.x / px(1.), position.y / px(1.))
                        })
                {
                    this.scroll.note_activity();
                    cx.notify();
                }
                this.send(PluginEvent::Wheel {
                    delta,
                    shift: event.modifiers.shift,
                    x: position.x / px(1.),
                    y: position.y / px(1.),
                });
                cx.stop_propagation();
            }))
            .child(
                canvas(
                    move |bounds, window, cx| {
                        let _ = prepaint.update(cx, |this, _| {
                            this.bounds = bounds;
                            if let Some(scene) = this.current_scene() {
                                let face = font(scene.font.clone());
                                let id = window.text_system().resolve_font(&face);
                                let cw = window
                                    .text_system()
                                    .advance(id, px(scene.font_size), 'M')
                                    .map(|s| s.width / px(1.))
                                    .unwrap_or(8.4);
                                let ch = (scene.font_size * 1.45).ceil();
                                let dimensions = (
                                    bounds.size.width / px(1.),
                                    bounds.size.height / px(1.),
                                    cw,
                                    ch,
                                );
                                if dimensions != this.last_size {
                                    this.last_size = dimensions;
                                    this.send(PluginEvent::Resize {
                                        width: dimensions.0,
                                        height: dimensions.1,
                                        cell_width: cw,
                                        cell_height: ch,
                                    });
                                }
                                this.scroll.update(
                                    scene
                                        .scroll
                                        .as_ref()
                                        .map(|info| rect_bounds(info.rect, bounds.origin))
                                        .unwrap_or(bounds),
                                    scene.scroll.clone(),
                                    this.active.clone().unwrap_or_default(),
                                );
                            }
                        });
                    },
                    move |bounds, _, window, cx| {
                        let _ = paint.update(cx, |this, cx| this.paint(bounds, window, cx));
                    },
                )
                .size_full(),
            );
        if let Some(info) = scene.as_ref().and_then(|s| s.scroll.as_ref()) {
            if self.scroll.visible(info) {
                let bar = Scrollbar::vertical(&self.scroll).viewport_from_layout();
                // While the guest's overlay is present, keep the native thumb fully visible.
                let bar = if info.hide_after_ms.is_some() {
                    bar.mode(ScrollbarMode::Always)
                } else {
                    bar
                };
                view = view.child(
                    div()
                        .absolute()
                        .left(px(info.rect.x))
                        .top(px(info.rect.y))
                        .w(px(info.rect.w))
                        .h(px(info.rect.h))
                        .child(bar),
                );
            }
        }
        // Declarative buttons use the host's regular native widget and return their stable ID.
        if let Some(scene) = &scene {
            for widget in &scene.widgets {
                if !widget.edit {
                    let id = widget.id.clone();
                    view = view.child(
                        div()
                            .absolute()
                            .left(px(widget.rect.x))
                            .top(px(widget.rect.y))
                            .w(px(widget.rect.w))
                            .h(px(widget.rect.h))
                            .child(
                                Button::new(SharedString::from(format!("widget-{id}")))
                                    .label(widget.label.clone())
                                    .on_click(
                                        cx.listener(move |this, _, _, _| this.command(id.clone())),
                                    ),
                            ),
                    );
                }
            }
            // Guest-owned invisible regions select the native resize cursor without painting.
            for region in &scene.column_resize_regions {
                if region.w > 0. && region.h > 0. {
                    view = view.child(
                        div()
                            .absolute()
                            .left(px(region.x))
                            .top(px(region.y))
                            .w(px(region.w))
                            .h(px(region.h))
                            .cursor_col_resize()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                                    // Forward the resize press once; the parent surface also listens.
                                    this.focus(window, cx);
                                    this.pointer(
                                        "down",
                                        event.position,
                                        0,
                                        event.click_count as u8,
                                        event.modifiers.shift,
                                    );
                                    cx.stop_propagation();
                                }),
                            )
                            .on_mouse_move(cx.listener(
                                |this, event: &gpui_kit::MouseMoveEvent, _, cx| {
                                    if event.pressed_button.is_some() {
                                        this.pointer(
                                            "move",
                                            event.position,
                                            0,
                                            1,
                                            event.modifiers.shift,
                                        );
                                        cx.stop_propagation();
                                    }
                                },
                            ))
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, event: &MouseUpEvent, _, cx| {
                                    this.pointer("up", event.position, 0, 1, event.modifiers.shift);
                                    cx.stop_propagation();
                                }),
                            ),
                    );
                }
            }
        }
        if let Some(edit) = &self.editing {
            if let Some(widget) = scene
                .as_ref()
                .and_then(|s| s.widgets.iter().find(|w| w.id == edit.id))
            {
                // Mount the input last so it owns the full tab, including the close and resize areas.
                view = view.child(
                    div()
                        .absolute()
                        .left(px(widget.rect.x))
                        .top(px(widget.rect.y))
                        .w(px(widget.rect.w))
                        .h(px(widget.rect.h))
                        .overflow_hidden()
                        // Editing clicks stay with GPUI Kit's input instead of reaching tab actions.
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|_, _: &MouseDownEvent, _, cx| cx.stop_propagation()),
                        )
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(|_, _: &MouseUpEvent, _, cx| cx.stop_propagation()),
                        )
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(|_, _: &MouseDownEvent, _, cx| cx.stop_propagation()),
                        )
                        .on_mouse_up(
                            MouseButton::Right,
                            cx.listener(|_, _: &MouseUpEvent, _, cx| cx.stop_propagation()),
                        )
                        .on_mouse_move(cx.listener(|_, _: &gpui_kit::MouseMoveEvent, _, cx| {
                            cx.stop_propagation()
                        }))
                        .child(
                            Input::new(&edit.input)
                                .appearance(false)
                                .bordered(false)
                                .focus_bordered(false)
                                .shadow_none(),
                        ),
                );
            }
        }
        if self.commands_open {
            let mut menu = v_flex()
                .absolute()
                .top_0()
                .right_0()
                .w(px(260.))
                .p_2()
                .bg(cx.theme().popover);
            for entry in &self.entries {
                if entry.enabled && Some(&entry.manifest.id) == self.active.as_ref() {
                    for command in &entry.manifest.commands {
                        if command.menu {
                            let plugin = entry.manifest.id.clone();
                            let id = command.id.clone();
                            menu = menu.child(
                                Button::new(SharedString::from(format!("{plugin}-{id}")))
                                    .label(command.title.clone())
                                    .ghost()
                                    .small()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.send_to(
                                            &plugin,
                                            PluginEvent::Command {
                                                id: id.clone(),
                                                cwd: None,
                                                text: None,
                                            },
                                        );
                                        this.commands_open = false;
                                        cx.notify();
                                    })),
                            );
                        }
                    }
                }
            }
            view = view.child(menu);
        }
        view.into_any_element()
    }
}
impl EntityInputHandler for ExtensionPanel {
    /// Native IME queries concern only uncommitted composition; committed text goes to the guest.
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let units: Vec<_> = self.composition.encode_utf16().collect();
        let end = range.end.min(units.len());
        let start = range.start.min(end);
        *adjusted = Some(start..end);
        Some(String::from_utf16_lossy(&units[start..end]))
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let end = self.composition.encode_utf16().count();
        Some(UTF16Selection {
            range: end..end,
            reversed: false,
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        (!self.composition.is_empty()).then(|| 0..self.composition.encode_utf16().count())
    }
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.composition.clear();
        cx.notify();
    }
    fn replace_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.composition.clear();
        self.send(PluginEvent::Text(text.into()));
        cx.notify();
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.composition = text.into();
        cx.notify();
    }
    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        self.current_scene()
            .map(|s| rect_bounds(s.cursor, self.bounds.origin))
    }
    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(0)
    }
}
