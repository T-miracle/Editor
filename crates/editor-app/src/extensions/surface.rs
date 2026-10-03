//! Plugin management and the native document surface share the editor's controls and theme.
use super::*;
use crate::ui::controls::ButtonCustomVariant;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::dialog::{DialogAction, DialogClose, DialogFooter};
use gpui_kit::rgb;
impl ExtensionPanel {
    /// Both a preview body and a source-only toolbar must deliver live theme and locale changes.
    pub(super) fn native_environment(&mut self, cx: &mut Context<Self>) -> protocol::Environment {
        let environment = environment(&self.workspace, cx);
        if self.last_theme.as_ref() != Some(&environment) {
            self.last_theme = Some(environment.clone());
            for entry in &self.entries {
                if entry.enabled && Some(&entry.manifest.id) == self.active.as_ref() {
                    self.send_to(
                        &entry.manifest.id,
                        None,
                        PluginEvent::Theme(environment.clone()),
                    );
                }
            }
        }
        environment
    }

    /// Preview content and source-mode overlays share one keyed native view and event ownership.
    pub(super) fn native_document(
        &mut self,
        mut document: protocol::ui::Document,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<crate::ui::plugin::PluginView> {
        document.editor_toolbar = None;
        let environment = self.native_environment(cx);
        if let Some(view) = &self.native_ui {
            view.update(cx, |view, cx| {
                view.update_document(document, environment, window, cx)
            });
        } else {
            let tx = self.worker.tx.clone();
            let plugin = self
                .active
                .clone()
                .expect("an approved document has an owner");
            let panel = self
                .surface_id
                .clone()
                .expect("an approved document has a surface");
            let visible = self.visible.clone();
            let epoch = self.instance_epoch;
            self.native_ui = Some(cx.new(|cx| {
                crate::ui::plugin::PluginView::new(
                    plugin.clone(),
                    document,
                    environment,
                    move |event, _| {
                        if visible.get() {
                            let _ = tx.send(Work::Event(
                                plugin.clone(),
                                epoch,
                                Some(panel.clone()),
                                PluginEvent::Ui(event),
                            ));
                        }
                    },
                    window,
                    cx,
                )
            }));
        }
        let key = format!(
            "{}/{}",
            self.active.as_deref().unwrap_or_default(),
            self.surface_id.as_deref().unwrap_or_default()
        );
        let view = self.native_ui.as_ref().unwrap().clone();
        view.update(cx, |view, cx| {
            view.update_images(&key, &self.images, window, cx)
        });
        view
    }

    /// Show package origin and requested capabilities above the manager's README.
    pub(super) fn open_install_dialog(
        &mut self,
        package: Package,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let executable =
            package.manifest.component.is_some() || !package.manifest.services.is_empty();
        let name = package.manifest.name.clone();
        let version = package.manifest.version.clone();
        let current = self
            .entries
            .iter()
            .find(|entry| entry.manifest.id == package.manifest.id)
            .map(|entry| entry.manifest.version.as_str());
        let action = match package_action(&version, current) {
            "已安装" => "重新安装",
            action => action,
        };
        let source = package.source.clone();
        let permissions = package.manifest.permissions.clone();
        let services = package.manifest.services.clone();
        let owner = cx.entity().downgrade();
        let install_owner = owner.clone();
        window.open_dialog(cx, move |dialog, _, cx| {
            let name = name.clone();
            let source = source.clone();
            let permissions = permissions.clone();
            let services = services.clone();
            let install_package = package.clone();
            let install_owner = install_owner.clone();
            let cancel_owner = owner.clone();
            dialog
                .title(format!("{action}插件 · {name} v{version}"))
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
                        format!("{action}后将允许以下能力：")
                    } else {
                        "此插件仅提供声明式资源，无需额外运行权限。".to_owned()
                    });
                    for permission in &permissions {
                        let network_image_permission = rust_i18n::t!("plugins.permission_network_images");
                        // This grant is scoped to host-owned image inputs, rather than arbitrary file writes.
                        let workspace_write_permission = rust_i18n::t!("plugins.permission_workspace_write");
                        let explanation = match permission.as_str() {
                            "assets.read" => "读取此插件安装包内的资源文件",
                            "process.exec" => "执行任意本机程序（含交互式终端）：以当前用户权限访问文件与网络，WASM 沙箱不限制这些程序",
                            "dependencies.prepare" => "下载、校验并解包插件声明或 WASM 钩子返回的服务依赖；保存在编辑器私有目录，不运行安装脚本、不修改全局 PATH",
                            "dependencies.install" => "依赖需要原生安装步骤时，另行展示程序、参数、目标和用途；您确认具体方案后才会执行，大型 SDK 还需主动勾选",
                            value if value.starts_with("process.service.") => "启动此包声明的固定本机服务：程序以当前用户权限运行，可访问本机文件与网络",
                            "workspace.read" => "读取当前工作区文件",
                            "workspace.write" => workspace_write_permission.as_ref(),
                            "network.images" => network_image_permission.as_ref(),
                            "clipboard" => "读写系统剪贴板",
                            "storage" => "保存插件私有配置与会话数据",
                            _ => permission,
                        };
                        details = details.child(format!("• {explanation}"));
                        if let Some(service) = permission.strip_prefix("process.service.")
                            .and_then(|id| services.get(id)) {
                            // Show the approved executable and argument vector separately from prose.
                            details = details.child(format!("  程序：{} · 参数：{:?}", service.program, service.args));
                            if let Some(plan) = &service.installation {
                                for artifact in &plan.artifacts {
                                    details = details.child(format!("  依赖：{} {} · {} · {:?} · SHA-256 {}", artifact.id, artifact.version, artifact.platform, artifact.source, artifact.sha256));
                                }
                            }
                        }
                    }
                    details = details.child(if executable {
                        if action == "安装" {
                            "安装后插件即可启动声明的程序。".to_owned()
                        } else {
                            "将停止插件当前运行的程序，保存会话后启动新程序。原有命令不会自动重跑。"
                                .to_owned()
                        }
                    } else {
                        format!("{action}后会加载语法、主题或图标资源。")
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
                                            .label(format!("确认{action}"))
                                            .primary()
                                            .when(action == "更新", |button| {
                                                button.custom(update_button_style(cx))
                                            })
                                            .outline(),
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

    /// Confirm the uninstall impact and keep both data-retention choices visible.
    pub(super) fn open_remove_dialog(
        &mut self,
        id: String,
        remove: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entry = self.entries.iter().find(|entry| entry.manifest.id == id);
        let name = entry
            .map(|entry| entry.manifest.name.clone())
            .unwrap_or_else(|| id.clone());
        let executable = entry.is_some_and(|entry| {
            entry.manifest.component.is_some() || !entry.manifest.services.is_empty()
        });
        let count = self.processes.get(&id).copied().unwrap_or(0);
        let impact = if executable {
            format!("将关闭 {count} 个运行中的程序。")
        } else {
            "将撤销此插件提供的语法、主题或图标资源。".to_owned()
        };
        let owner = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, cx| {
            let preserve_owner = owner.clone();
            let delete_owner = owner.clone();
            let cancel_owner = owner.clone();
            let preserve_id = id.clone();
            let delete_id = id.clone();
            let impact = impact.clone();
            dialog
                .title(format!(
                    "{}插件 · {name}",
                    if remove { "卸载" } else { "停用" }
                ))
                .width(px(520.))
                .overlay_closable(false)
                .close_button(false)
                .content(move |content, _, cx| {
                    content.child(
                        v_flex()
                            .id("plugin-remove-consent")
                            .debug_selector(|| "plugin-remove-consent".into())
                            .gap_3()
                            .child(impact.clone())
                            .when(remove, |details| {
                                details.child(
                                    div()
                                        .text_color(cx.theme().muted_foreground)
                                        .child("可保留插件配置，也可同时删除插件保存的数据。"),
                                )
                            }),
                    )
                })
                // Base Dialog renders these explicit choices in its visible footer.
                .footer(
                    DialogFooter::new()
                        .child(
                            div()
                                .id("plugin-remove-cancel")
                                .debug_selector(|| "plugin-remove-cancel".into())
                                .child(DialogClose::new().trigger(|button| button.label("取消"))),
                        )
                        .child(
                            div()
                                .id("plugin-remove-preserve")
                                .debug_selector(|| "plugin-remove-preserve".into())
                                .child(
                                    DialogAction::new().child(
                                        Button::new("confirm-plugin-preserve")
                                            .label(if remove {
                                                "卸载，保留数据"
                                            } else {
                                                "确认停用"
                                            })
                                            .primary()
                                            .when(remove, |button| {
                                                button.custom(uninstall_button_style(cx))
                                            })
                                            .outline(),
                                    ),
                                ),
                        )
                        .when(remove, |footer| {
                            footer.child(
                                div()
                                    .id("plugin-remove-delete")
                                    .debug_selector(|| "plugin-remove-delete".into())
                                    .child(
                                        Button::new("confirm-plugin-delete")
                                            .label("卸载并删除数据")
                                            .custom(uninstall_button_style(cx))
                                            .outline()
                                            .on_click(move |_, window, cx| {
                                                let queued = delete_owner
                                                    .update(cx, |this, cx| {
                                                        let queued =
                                                            this.queue_lifecycle(Work::Uninstall(
                                                                delete_id.clone(),
                                                                true,
                                                            ));
                                                        if queued {
                                                            this.confirm = None;
                                                            this.confirm_dialog_open = false;
                                                            cx.notify();
                                                        }
                                                        queued
                                                    })
                                                    .unwrap_or(false);
                                                if queued {
                                                    window.close_dialog(cx);
                                                }
                                            }),
                                    ),
                            )
                        }),
                )
                .on_ok(move |_, _, cx| {
                    preserve_owner
                        .update(cx, |this, cx| {
                            let queued = if remove {
                                this.queue_lifecycle(Work::Uninstall(preserve_id.clone(), false))
                            } else {
                                this.worker
                                    .tx
                                    .send(Work::Disable(preserve_id.clone()))
                                    .is_ok()
                            };
                            if queued {
                                this.confirm = None;
                                this.confirm_dialog_open = false;
                                cx.notify();
                            }
                            queued
                        })
                        .unwrap_or(false)
                })
                .on_cancel(move |_, _, cx| {
                    let _ = cancel_owner.update(cx, |this, cx| {
                        this.confirm = None;
                        this.confirm_dialog_open = false;
                        cx.notify();
                    });
                    true
                })
        });
    }
}

/// Amber accents distinguish upgrade outlines and their subtle interaction states.
pub(super) fn update_button_style(cx: &App) -> ButtonCustomVariant {
    let (background, foreground, hover, active) = if cx.theme().is_dark() {
        (0x524234, 0xffd29d, 0x614c39, 0x705740)
    } else {
        (0xfff0d9, 0x9c570d, 0xffe5bd, 0xffd99f)
    };
    ButtonCustomVariant::new(cx)
        .color(rgb(background).into())
        .foreground(rgb(foreground).into())
        .hover(rgb(hover).into())
        .active(rgb(active).into())
}

/// Rose accents keep removal outlines readable in both editor themes.
pub(super) fn uninstall_button_style(cx: &App) -> ButtonCustomVariant {
    let (background, foreground, hover, active) = if cx.theme().is_dark() {
        (0x553b43, 0xffbdc9, 0x65424b, 0x764953)
    } else {
        (0xffe6ea, 0xb93851, 0xffd3dc, 0xffc1ce)
    };
    ButtonCustomVariant::new(cx)
        .color(rgb(background).into())
        .foreground(rgb(foreground).into())
        .hover(rgb(hover).into())
        .active(rgb(active).into())
}

/// Compare versions for update availability and explicitly chosen local-package confirmations.
pub(super) fn package_action(available: &str, installed: Option<&str>) -> &'static str {
    let Some(current) = installed else {
        return "安装";
    };
    match (
        semver::Version::parse(available),
        semver::Version::parse(current),
    ) {
        (Ok(available), Ok(installed)) if available > installed => "更新",
        (Ok(available), Ok(installed)) if available < installed => "降级安装",
        _ => "已安装",
    }
}

impl Render for ExtensionPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.manager_open || self.surface_id.is_none() {
            return self.manager(window, cx);
        }
        // Every retained callback belongs to this publication, not a future replacement.
        let epoch = self.instance_epoch;
        if self._focus_events.is_empty() {
            self._focus_events
                .push(cx.on_focus(&self.focus, window, move |this, _, _| {
                    if this.instance_epoch != epoch {
                        return;
                    }
                    this.send(PluginEvent::Focus(true))
                }));
            self._focus_events
                .push(cx.on_blur(&self.focus, window, move |this, _, cx| {
                    if this.instance_epoch != epoch {
                        return;
                    }
                    this.send(PluginEvent::Focus(false));
                    cx.notify();
                }));
        }
        self.native_environment(cx);
        if let Some(error) = &self.preview_error {
            // Host-side admission failures are readable UI, with no stale guest input target underneath.
            return div()
                .size_full()
                .p_4()
                .text_color(cx.theme().muted_foreground)
                .debug_selector(|| "plugin-preview-error".into())
                .child(error.clone())
                .into_any_element();
        }
        if let Some(document) = self.current_document().map(|document| (*document).clone()) {
            let view = self.native_document(document, window, cx);
            return div()
                .size_full()
                .key_context("PluginSurface")
                .child(view)
                .children(self.command_popup(window, cx))
                .into_any_element();
        }
        self.native_ui = None;
        // An unpublished or retired document has no focusable guest controls.
        div()
            .size_full()
            .children(self.command_popup(window, cx))
            .into_any_element()
    }
}
