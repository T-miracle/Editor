//! Plugin management and the native document surface share the editor's controls and theme.
use super::*;
use crate::ui::controls::ButtonCustomVariant;
use crate::ui::controls::Checkbox;
use crate::ui::controls::SegmentedTabs;
use crate::ui::controls::tab_strip;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::dialog::{DialogAction, DialogClose, DialogFooter};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::rgb;
impl ExtensionPanel {
    /// Show package origin and requested capabilities above the manager's README.
    fn open_install_dialog(
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
                        let explanation = match permission.as_str() {
                            "assets.read" => "读取此插件安装包内的资源文件",
                            "process.exec" => "执行任意本机程序（含交互式终端）：以当前用户权限访问文件与网络，WASM 沙箱不限制这些程序",
                            "dependencies.prepare" => "下载、校验并解包插件声明或 WASM 钩子返回的服务依赖；保存在编辑器私有目录，不运行安装脚本、不修改全局 PATH",
                            "dependencies.install" => "依赖需要原生安装步骤时，另行展示程序、参数、目标和用途；您确认具体方案后才会执行，大型 SDK 还需主动勾选",
                            value if value.starts_with("process.service.") => "启动此包声明的固定本机服务：程序以当前用户权限运行，可访问本机文件与网络",
                            "workspace.read" => "读取当前工作区文件",
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
    fn open_remove_dialog(
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

    /// Install and destructive lifecycle actions have explicit, reviewable native controls.
    fn manager(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if self.manager_search.is_none() {
            // Installed details need available versions before the user visits the market tab.
            if self.manager_packages.is_empty() {
                self.load_market_packages();
            }
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
        if self.installation.is_some() && !self.installation_dialog_open {
            self.installation_dialog_open = true;
            cx.defer_in(window, |this, window, cx| {
                this.open_installation_progress(window, cx)
            });
        }
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
        if let Some((id, remove)) = self.confirm.clone().filter(|_| !self.confirm_dialog_open) {
            // The confirmation opens after the manager has finished this render pass.
            self.confirm_dialog_open = true;
            cx.defer_in(window, move |this, window, cx| {
                if this.confirm.as_ref() == Some(&(id.clone(), remove)) {
                    this.open_remove_dialog(id, remove, window, cx);
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
                        this.confirm_dialog_open = false;
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
                            .child(tab_strip(
                                "plugin-manager-tabs",
                                usize::from(self.manager_market),
                                ["已安装", "插件市场"],
                                {
                                    let owner = cx.entity().downgrade();
                                    move |index, _, cx| {
                                        let _ = owner.update(cx, |this, cx| {
                                            let market = index == 1;
                                            if market && !this.manager_market {
                                                this.load_market_packages();
                                            }
                                            this.manager_market = market;
                                            this.manager_selected = None;
                                            cx.notify();
                                        });
                                    }
                                },
                                cx,
                            )),
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
            // Installed preferences survive the protocol transition, but cannot authorize an old component.
            let incompatibility = selected_entry.and_then(Installed::compatibility_error);
            let incompatible = incompatibility.is_some();
            let installed_version = selected_entry.map(|entry| entry.manifest.version.as_str());
            let package_path = selected_package
                .and_then(|package| package.source.as_ref())
                .map(PathBuf::from);
            // Installation and updates have distinct controls; older packages expose no downgrade action.
            let can_install = selected_entry.is_none();
            let can_update = selected_package.is_some_and(|package| {
                package_action(&package.manifest.version, installed_version) == "更新"
                    || (incompatible
                        && selected_entry.is_some_and(|entry| entry.digest != package.digest))
            });
            let global_enabled =
                selected_entry.is_some_and(|entry| entry.global_enabled.unwrap_or(entry.enabled));
            let project_enabled = selected_entry
                .is_some_and(|entry| entry.project_enabled_in(&self.workspace.to_string_lossy()));
            let uninstall_id = id.clone();
            // Package inspection belongs to install/update; removal has its own loading state.
            let install_loading = self.progress.as_ref().is_some_and(|progress| {
                progress.action == LifecycleAction::Inspect
                    || (progress.action == LifecycleAction::Install && progress.id == id)
            });
            let uninstall_loading = self.progress.as_ref().is_some_and(|progress| {
                progress.action == LifecycleAction::Uninstall && progress.id == id
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
                            .when(can_install, |row| {
                                let package_path = package_path.clone();
                                row.child(
                                    div()
                                        .id("plugin-install-action-region")
                                        .debug_selector(|| "plugin-install-action-region".into())
                                        .child(
                                            Button::new("plugin-install-action")
                                                .label("安装")
                                                .primary()
                                                .outline()
                                                .loading(install_loading)
                                                .disabled(busy || package_path.is_none())
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    if let Some(path) = package_path.clone() {
                                                        this.queue_lifecycle(Work::Inspect(path));
                                                    }
                                                    cx.notify();
                                                })),
                                        ),
                                )
                            })
                            .when(can_update, |row| {
                                let package_path = package_path.clone();
                                row.child(
                                    div()
                                        .id("plugin-update-action-region")
                                        .debug_selector(|| "plugin-update-action-region".into())
                                        .child(
                                            Button::new("plugin-update-action")
                                                .label("更新")
                                                .custom(update_button_style(cx))
                                                .outline()
                                                .loading(install_loading)
                                                .disabled(busy || package_path.is_none())
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    if let Some(path) = package_path.clone() {
                                                        this.queue_lifecycle(Work::Inspect(path));
                                                    }
                                                    cx.notify();
                                                })),
                                        ),
                                )
                            })
                            .when(selected_entry.is_some(), |row| {
                                row.child(
                                    div()
                                        .id("plugin-uninstall-action-region")
                                        .debug_selector(|| "plugin-uninstall-action-region".into())
                                        .child(
                                            Button::new("plugin-uninstall-action")
                                                .label("卸载")
                                                .custom(uninstall_button_style(cx))
                                                .outline()
                                                .loading(uninstall_loading)
                                                .disabled(busy)
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.confirm =
                                                        Some((uninstall_id.clone(), true));
                                                    this.confirm_dialog_open = false;
                                                    cx.notify();
                                                })),
                                        ),
                                )
                            })
                            .when_some(
                                selected_id.clone().zip(selected_global),
                                |row, (id, enabled)| {
                                    let owner = cx.entity().downgrade();
                                    row.child(
                                        div()
                                            .id("plugin-global-scope-region")
                                            .debug_selector(|| "plugin-global-scope-region".into())
                                            .w(px(176.))
                                            .child(
                                                SegmentedTabs::new("plugin-global-scope")
                                                    .selected_index(usize::from(!enabled))
                                                    .labels(["全局启动", "全局禁用"])
                                                    .disabled(busy || incompatible)
                                                    .on_change(move |index, _, cx| {
                                                        let enable = index == 0;
                                                        if busy || incompatible || enable == enabled
                                                        {
                                                            return;
                                                        }
                                                        let _ = owner.update(cx, |this, cx| {
                                                            this.queue_lifecycle(if enable {
                                                                Work::Enable(id.clone())
                                                            } else {
                                                                Work::Disable(id.clone())
                                                            });
                                                            cx.notify();
                                                        });
                                                    }),
                                            ),
                                    )
                                },
                            )
                            // Project overrides follow the global choice in the same row.
                            .when(selected_entry.is_some() && !global_enabled, |row| {
                                row.child(
                                    div()
                                        .id("plugin-project-scope-region")
                                        .debug_selector(|| "plugin-project-scope-region".into())
                                        .child(
                                            Checkbox::new("plugin-project-enabled")
                                                .label("本项目启用")
                                                .checked(project_enabled)
                                                .disabled(busy || incompatible)
                                                .on_change(move |checked, _, cx| {
                                                    if busy || incompatible {
                                                        return;
                                                    }
                                                    let checked = *checked;
                                                    let _ = project_owner.update(cx, |this, cx| {
                                                        let _ = this.worker.tx.send(
                                                            Work::SetProjectEnabled(
                                                                project_id.clone(),
                                                                checked,
                                                            ),
                                                        );
                                                        cx.notify();
                                                    });
                                                }),
                                        ),
                                )
                            }),
                    ),
            );
            if let Some(reason) = incompatibility {
                // This persistent status is independent of transient operation errors and README loading.
                detail = detail.child(
                    div()
                        .debug_selector(|| "plugin-incompatible-status".into())
                        .px_5()
                        .py_3()
                        .text_color(cx.theme().danger)
                        .child(format!(
                            "不兼容，请更新。{reason} 原有设置和启用范围已保留。"
                        )),
                );
            }
            if selected_entry.is_some() && !incompatible {
                detail = detail.child(self.recovery_controls(&id, busy, cx));
            }
            if self.status.is_none() {
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
                        .child(ui::controls::markdown_view(
                            "plugin-readme",
                            readme.unwrap_or_else(|| "此插件没有提供 README.md。".into()),
                            typography::font_size(cx),
                            cx,
                        )),
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

/// Amber accents distinguish upgrade outlines and their subtle interaction states.
fn update_button_style(cx: &App) -> ButtonCustomVariant {
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
fn uninstall_button_style(cx: &App) -> ButtonCustomVariant {
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
        let environment = environment(&self.workspace, cx);
        // Plugin-only theme tokens trigger the same live update as editor base colors.
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
            if let Some(view) = &self.native_ui {
                view.update(cx, |view, cx| {
                    view.update_document(document, environment, window, cx)
                });
            } else {
                let tx = self.worker.tx.clone();
                let plugin = self.active.clone().unwrap();
                let panel = self.surface_id.clone().unwrap();
                let visible = self.visible.clone();
                self.native_ui = Some(cx.new(|cx| {
                    crate::ui::plugin::PluginView::new(
                        plugin.clone(),
                        document,
                        environment,
                        move |event, _| {
                            if !visible.get() {
                                return;
                            }
                            let _ = tx.send(Work::Event(
                                plugin.clone(),
                                epoch,
                                Some(panel.clone()),
                                PluginEvent::Ui(event),
                            ));
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
            self.native_ui
                .as_ref()
                .unwrap()
                .update(cx, |view, cx| view.update_images(&key, &self.images, cx));
            return div()
                .size_full()
                .key_context("PluginSurface")
                .child(self.native_ui.as_ref().unwrap().clone())
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
