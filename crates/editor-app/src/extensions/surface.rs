//! Generic scene painting, native IME, shared scrollbars and package management controls.
use super::*;
use gpui_base::{Disableable, Scrollbar, ScrollbarHandle, ScrollbarMode};
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
                protocol::Paint::Fill { rect, color } => {
                    window.paint_quad(fill(rect_bounds(*rect, bounds.origin), rgb(*color)));
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
    /// Install and destructive lifecycle actions have explicit, reviewable native controls.
    fn manager(&self, cx: &mut Context<Self>) -> AnyElement {
        let busy = self.progress.is_some();
        let installing = self
            .progress
            .as_ref()
            .is_some_and(|p| p.action == LifecycleAction::Install);
        let mut content = v_flex()
            .id("runtime-plugin-manager")
            .debug_selector(|| "runtime-plugin-manager".into())
            .size_full()
            .p_4()
            .gap_2()
            .overflow_y_scroll()
            .bg(cx.theme().background)
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("install-local-plugin")
                            .label("安装 / 更新本机插件包")
                            .when(installing, |button| button.icon(IconName::Loader))
                            .loading(installing)
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| this.choose_package(cx))),
                    )
                    .child(
                        Button::new("load-bundled-plugins")
                            .label("查看随附插件")
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| {
                                let exe = std::env::current_exe()
                                    .ok()
                                    .and_then(|p| p.parent().map(Path::to_owned));
                                let roots = exe.into_iter().map(|p| p.join("plugins")).chain(
                                    std::iter::once(
                                        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                                            .join("../../dist/plugins"),
                                    ),
                                );
                                this.bundled.clear();
                                // Bundled plugin packages use the standard ZIP suffix.
                                for root in roots {
                                    if let Ok(files) = std::fs::read_dir(root) {
                                        this.bundled.extend(
                                            files.flatten().map(|f| f.path()).filter(|p| {
                                                p.extension().is_some_and(|e| e == "zip")
                                            }),
                                        );
                                    }
                                }
                                this.bundled.sort();
                                this.bundled.dedup();
                                cx.notify();
                            })),
                    ),
            );
        for (index, path) in self.bundled.iter().enumerate() {
            let path = path.clone();
            // Show the plugin package name without exposing the ZIP file extension in the list.
            let label = format!(
                "安装 {}",
                path.file_stem().unwrap_or_default().to_string_lossy()
            );
            content = content.child(
                Button::new(("bundled-package", index))
                    .label(label)
                    .disabled(busy)
                    .on_click(cx.listener(move |this, _, _, _| {
                        let _ = this.worker.tx.send(Work::Inspect(path.clone()));
                    })),
            );
        }
        for entry in &self.entries {
            let id = entry.manifest.id.clone();
            let enable_id = id.clone();
            let remove_id = id.clone();
            let enabled = entry.enabled;
            let enabling = self
                .progress
                .as_ref()
                .is_some_and(|p| p.action == LifecycleAction::Enable && p.id == id);
            let uninstalling = self
                .progress
                .as_ref()
                .is_some_and(|p| p.action == LifecycleAction::Uninstall && p.id == id);
            content = content.child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(format!(
                        "{}  {}  {}",
                        entry.manifest.name,
                        entry.manifest.version,
                        if enabled { "已启用" } else { "已停用" }
                    ))
                    .child(
                        Button::new(SharedString::from(format!("enable-{id}")))
                            .label(if enabled { "停用" } else { "启用" })
                            .when(enabling, |button| button.icon(IconName::Loader))
                            .loading(enabling)
                            .disabled(busy)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if enabled {
                                    this.confirm = Some((enable_id.clone(), false));
                                } else {
                                    this.queue_lifecycle(Work::Enable(enable_id.clone()));
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("remove-{id}")))
                            .label("卸载")
                            .when(uninstalling, |button| button.icon(IconName::Loader))
                            .loading(uninstalling)
                            .disabled(busy)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.confirm = Some((remove_id.clone(), true));
                                cx.notify();
                            })),
                    ),
            );
            if let Some(error) = &entry.error {
                content = content.child(div().text_color(cx.theme().danger).child(error.clone()));
            }
        }
        if let Some(package) = &self.pending {
            let executable = package.manifest.component.is_some();
            if let Some(source) = &package.source {
                content = content.child(format!("来源：{source}"));
            }
            content = content.child(div().child(format!(
                    "本机未签名插件：{} {}（{}）",
                    package.manifest.name,
                    package.manifest.version,
                    package.manifest.id.strip_prefix("me.").unwrap_or(&package.manifest.id)
                )));
            // Resource-only packages never start guest code or request runtime capabilities.
            content = content.child(if executable {
                "允许以下能力后立即安装或更新："
            } else {
                "此插件仅提供声明式资源，无需额外运行权限。"
            });
            for permission in &package.manifest.permissions {
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
                content = content.child(format!("• {explanation}"));
            }
            content = content
                .child(if executable {
                    "更新将停止插件当前运行的程序，保存会话后启动新程序。原有命令不会自动重跑。"
                } else {
                    "更新后会重新加载语法、主题或图标资源。"
                })
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("confirm-plugin-install")
                                .label(if executable {
                                    "允许并安装 / 更新"
                                } else {
                                    "安装 / 更新"
                                })
                                .when(installing, |button| button.icon(IconName::Loader))
                                .loading(installing)
                                .disabled(busy)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(package) = this.pending.clone() {
                                        this.queue_lifecycle(Work::Install(package));
                                    }
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("cancel-plugin-install")
                                .label("取消")
                                .disabled(busy)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.pending = None;
                                    cx.notify();
                                })),
                        ),
                );
        }
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
        content.into_any_element()
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
            return self.manager(cx);
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
        let theme = vec![
            environment.background,
            environment.foreground,
            environment.muted,
            environment.border,
            environment.accent,
            environment.selection,
        ];
        if theme != self.last_theme {
            self.last_theme = theme;
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
        if let Some(edit) = &self.editing {
            if let Some(widget) = scene
                .as_ref()
                .and_then(|s| s.widgets.iter().find(|w| w.id == edit.id))
            {
                view = view.child(
                    div()
                        .absolute()
                        .left(px(widget.rect.x))
                        .top(px(widget.rect.y))
                        .w(px(widget.rect.w))
                        .h(px(widget.rect.h))
                        .child(Input::new(&edit.input)),
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
