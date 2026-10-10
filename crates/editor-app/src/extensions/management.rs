//! Native plugin management is separate from plugin-owned document surfaces and consent overlays.
use super::surface::{package_action, uninstall_button_style, update_button_style};
use super::*;
use crate::ui::controls::{Checkbox, SegmentedTabs, tab_strip, vertical_viewport_scrollbar};
use gpui_kit::component::scroll::ScrollableElement;

/// Available detail pages; disabled market tabs never become stored selection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum DetailTab {
    #[default]
    Overview,
    RuntimeLog,
}
impl DetailTab {
    /// Map only supported tab positions; placeholders remain unavailable by keyboard and mouse.
    pub(super) fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::Overview),
            5 => Some(Self::RuntimeLog),
            _ => None,
        }
    }
    fn index(self) -> usize {
        match self {
            Self::Overview => 0,
            Self::RuntimeLog => 5,
        }
    }
}

impl ExtensionPanel {
    /// Install and destructive lifecycle actions have explicit, reviewable native controls.
    pub(super) fn manager(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if self.manager_search.is_none() {
            // Installed details need available versions before the user visits the market tab.
            if self.manager_packages.is_empty() {
                self.load_market_packages();
            }
            // Keep search input state alive across manager repaints and focus changes.
            let input = cx.new(|cx| {
                InputState::new(window, cx).placeholder(t!("plugins.search").to_string())
            });
            self.manager_search_subscription =
                Some(cx.subscribe(&input, |this, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.manager_selected = None;
                        this.manager_detail_tab = DetailTab::Overview;
                        this.manager_detail_scroll.set_offset(Default::default());
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
        if self.manager_market {
            return self.online_market(window, cx);
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
                    // The sessions this plugin is serving are read from the editor, which owns the run
                    // controls, so the user sees what the change would take away before deciding.
                    let extra = this.parent.upgrade().and_then(|parent| {
                        let debug_provider = parent
                            .read(cx)
                            .run_controls
                            .debug_availability()
                            .ok()
                            .map(str::to_owned);
                        let impact = parent
                            .read(cx)
                            .run_controls
                            .plugin_session_impact(&id, debug_provider.as_deref());
                        impact.summary()
                    });
                    this.open_remove_dialog(id, remove, extra, window, cx);
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
        // Store the resolved row so delayed log acknowledgements can verify the exact displayed plugin.
        self.manager_selected = selected_id.clone();
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
                    .debug_selector({
                        let id = id.clone();
                        move || format!("plugin-row-{id}").into()
                    })
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
                        this.manager_detail_tab = DetailTab::Overview;
                        this.manager_detail_scroll.set_offset(Default::default());
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
                    .gap_2()
                    // Keep a stable hit target for native search and summary-to-log routing tests.
                    .child(
                        div()
                            .debug_selector(|| "plugin-manager-search".into())
                            .w_full()
                            .child(Input::new(self.manager_search.as_ref().unwrap())),
                    )
                    .child(
                        div()
                            .id("plugin-manager-tab-strip")
                            .debug_selector(|| "plugin-manager-tab-strip".into())
                            .child(tab_strip(
                                "plugin-manager-tabs",
                                usize::from(self.manager_market),
                                [
                                    (t!("plugins.installed").into(), false),
                                    (t!("plugins.market").into(), false),
                                ],
                                [None; 2],
                                &self.manager_tabs_focus,
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
                                            this.manager_detail_tab = DetailTab::Overview;
                                            this.manager_detail_scroll
                                                .set_offset(Default::default());
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
                    .px_2()
                    .py_1()
                    .border_t_1()
                    .border_color(cx.theme().sidebar_border)
                    .child(
                        Button::new("install-local-plugin")
                            .label(t!("plugins.install_local").to_string())
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
            .min_h(px(0.))
            .overflow_hidden()
            .bg(cx.theme().background);
        // Manager-level failures remain visible even if no plugin could be loaded.
        if let Some(status) = self
            .status
            .as_ref()
            .filter(|status| status.plugin.is_none())
        {
            detail = detail.child(
                div()
                    .debug_selector(|| "plugin-manager-operation-error".into())
                    .p_3()
                    .flex_shrink_0()
                    .text_color(cx.theme().danger)
                    .child(t!("plugins.manager_error", error = status.message.clone()).to_string()),
            );
        }
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
                    .id("plugin-manager-header")
                    .debug_selector(|| "plugin-manager-header".into())
                    .p_5()
                    .gap_4()
                    .flex_shrink_0()
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
                            // Preserve action order when the window narrows or interface text grows.
                            .flex_wrap()
                            .items_center()
                            .when(can_install, |row| {
                                let package_path = package_path.clone();
                                row.child(
                                    div()
                                        .id("plugin-install-action-region")
                                        .debug_selector(|| "plugin-install-action-region".into())
                                        .child(
                                            Button::new("plugin-install-action")
                                                .label(t!("plugins.install").to_string())
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
                                                .label(t!("plugins.update").to_string())
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
                                                .label(t!("plugins.uninstall").to_string())
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
                            .when(selected_entry.is_some() && !incompatible, |row| {
                                row.child(self.restart_action(&id, busy, cx))
                            })
                            .when_some(
                                selected_id.clone().zip(selected_global),
                                |row, (id, enabled)| {
                                    let owner = cx.entity().downgrade();
                                    row.child(
                                        div()
                                            .id("plugin-global-scope-region")
                                            .debug_selector(|| "plugin-global-scope-region".into())
                                            // Equal options grow with the same interface font used by Root.
                                            .w(px(176.) * (cx.theme().font_size / px(14.)))
                                            .flex_shrink_0()
                                            .child(
                                                SegmentedTabs::new("plugin-global-scope")
                                                    .selected_index(usize::from(!enabled))
                                                    .labels([
                                                        t!("plugins.global_enabled").to_string(),
                                                        t!("plugins.global_disabled").to_string(),
                                                    ])
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
                                                .label(t!("plugins.project_enabled").to_string())
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
            let owner = cx.entity().downgrade();
            detail = detail.child(tab_strip(
                "plugin-detail-tabs",
                self.manager_detail_tab.index(),
                [
                    (t!("plugins.overview").into(), false),
                    (t!("plugins.changes").into(), true),
                    (t!("plugins.reviews").into(), true),
                    (t!("plugins.versions").into(), true),
                    (t!("plugins.information").into(), true),
                    (t!("plugins.runtime_log").into(), false),
                ],
                [None, None, None, None, None, self.log_badge(&id)],
                &self.manager_detail_focus,
                move |index, _, cx| {
                    if let Some(tab) = DetailTab::from_index(index) {
                        let _ = owner.update(cx, |this, cx| {
                            this.manager_detail_tab = tab;
                            this.manager_detail_scroll.set_offset(Default::default());
                            cx.notify();
                        });
                    }
                },
                cx,
            ));
            let mut body = v_flex().w_full().min_w(px(0.));
            if self.manager_detail_tab == DetailTab::Overview {
                self.manager_log_view = None;
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
                body = body.child(
                    div()
                        .id("plugin-readme-region")
                        .debug_selector(|| "plugin-readme-region".into())
                        .p_5()
                        .child(ui::controls::markdown_view(
                            "plugin-readme",
                            readme.unwrap_or_else(|| t!("plugins.no_readme").to_string()),
                            typography::font_size(cx),
                            cx,
                        )),
                );
            } else {
                body = body.child(self.runtime_status(&id, cx));
            }
            // The constrained viewport owns scrolling and the local Base scrollbar; its siblings stay fixed.
            detail = detail.child(
                div()
                    .relative()
                    .flex_1()
                    .min_h(px(0.))
                    .w_full()
                    .child(
                        div()
                            .id("plugin-detail-scroll")
                            .debug_selector(|| "plugin-manager-detail-content".into())
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.manager_detail_scroll)
                            .child(body),
                    )
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .child(vertical_viewport_scrollbar(&self.manager_detail_scroll, cx)),
                    ),
            );
        } else {
            detail = detail.child(div().p_5().text_color(cx.theme().muted_foreground).child(
                if self.manager_market {
                    t!("plugins.empty_market").to_string()
                } else {
                    t!("plugins.empty_installed").to_string()
                },
            ));
        }
        let content = detail;
        // Transient worker errors belong to the runtime page, not below the selected content.
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
