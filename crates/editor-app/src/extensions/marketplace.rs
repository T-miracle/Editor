//! Native marketplace uses inert catalog snapshots; consent never constructs a fake Package.
use super::*;
use crate::ui::controls::vertical_viewport_scrollbar;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::dialog::{DialogAction, DialogClose, DialogFooter};
use gpui_kit::component::scroll::ScrollableElement;
use plugin_runtime::marketplace::{CATALOG_URL, Release, Sort};

/// Local filters survive repaints. The worker owns catalog generation and network results.
#[derive(Default)]
pub(super) struct ViewState {
    pub revision: u64,
    started: bool,
    category: String,
    tag: String,
    sort: Sort,
}

impl ExtensionPanel {
    /// Start one refresh per market-window lifetime; further attempts require the refresh button.
    fn refresh_online_market(&mut self) {
        self.market_view.started = true;
        self.worker.refresh_market(
            self.root.join("marketplace/catalog.json"),
            CATALOG_URL.into(),
        );
    }

    /// Search, filters and details use local controls and the existing native scroll viewport.
    pub(super) fn online_market(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if !self.market_view.started {
            self.market_view.started = true;
            if self.worker.state.lock().unwrap().market.generation() == 0 {
                self.refresh_online_market();
            }
        }
        let state = self.worker.state.lock().unwrap().market.clone();
        let query = self
            .manager_search
            .as_ref()
            .map(|input| input.read(cx).value().to_string())
            .unwrap_or_default();
        let rows = state
            .catalog
            .as_ref()
            .map(|catalog| {
                catalog.search(
                    &query,
                    &self.market_view.category,
                    &self.market_view.tag,
                    self.market_view.sort,
                )
            })
            .unwrap_or_default();
        let selection = self
            .manager_selected
            .as_ref()
            .filter(|id| rows.iter().any(|r| &r.id == *id))
            .cloned()
            .or_else(|| rows.first().map(|r| r.id.clone()));
        self.manager_selected = selection.clone();
        let selected = rows
            .iter()
            .find(|r| Some(&r.id) == selection.as_ref())
            .copied();
        let mut list = v_flex().gap_1();
        for row in rows {
            let id = row.id.clone();
            list = list.child(
                Button::new(SharedString::from(format!("market-row-{id}")))
                    .label(row.versions[0].manifest.name.clone())
                    .w_full()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.manager_selected = Some(id.clone());
                        this.manager_detail_scroll.set_offset(Default::default());
                        cx.notify();
                    })),
            );
        }
        let categories = state
            .catalog
            .as_ref()
            .map(|c| {
                c.plugins
                    .iter()
                    .map(|p| p.category.clone())
                    .collect::<std::collections::BTreeSet<_>>()
            })
            .unwrap_or_default();
        let tags = state
            .catalog
            .as_ref()
            .map(|c| {
                c.plugins
                    .iter()
                    .flat_map(|p| p.tags.iter().cloned())
                    .collect::<std::collections::BTreeSet<_>>()
            })
            .unwrap_or_default();
        let category_label = if self.market_view.category.is_empty() {
            t!("plugins.market_all_categories").to_string()
        } else {
            self.market_view.category.clone()
        };
        let tag_label = if self.market_view.tag.is_empty() {
            t!("plugins.market_all_tags").to_string()
        } else {
            self.market_view.tag.clone()
        };
        let mut sidebar = v_flex()
            .w(px(258.))
            .h_full()
            .gap_2()
            .p_3()
            .bg(cx.theme().sidebar)
            .child(
                Button::new("market-installed")
                    .label(t!("plugins.installed").to_string())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.manager_market = false;
                        this.manager_selected = None;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .debug_selector(|| "market-search".into())
                    .child(Input::new(self.manager_search.as_ref().unwrap())),
            )
            .child(
                Button::new("market-category")
                    .label(category_label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.market_view.category =
                            next_filter(&this.market_view.category, &categories);
                        this.manager_selected = None;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("market-tag")
                    .label(tag_label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.market_view.tag = next_filter(&this.market_view.tag, &tags);
                        this.manager_selected = None;
                        cx.notify();
                    })),
            );
        let sorts = [Sort::Relevance, Sort::Downloads, Sort::Updated, Sort::Name];
        sidebar = sidebar
            .child(crate::ui::controls::tab_strip(
                "market-sort",
                sorts
                    .iter()
                    .position(|s| *s == self.market_view.sort)
                    .unwrap_or_default(),
                [
                    (t!("plugins.market_sort_relevance").into(), false),
                    (t!("plugins.market_sort_downloads").into(), false),
                    (t!("plugins.market_sort_updated").into(), false),
                    (t!("plugins.market_sort_name").into(), false),
                ],
                [None; 4],
                &self.manager_tabs_focus,
                {
                    let owner = cx.entity().downgrade();
                    move |index, _, cx| {
                        let _ = owner.update(cx, |this, cx| {
                            this.market_view.sort = sorts[index];
                            cx.notify();
                        });
                    }
                },
                cx,
            ))
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scrollbar()
                    .child(list),
            )
            .child(
                Button::new("market-refresh")
                    .label(t!("plugins.market_refresh").to_string())
                    .disabled(state.refreshing)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.refresh_online_market();
                        cx.notify();
                    })),
            )
            .child(
                Button::new("market-local")
                    .label(t!("plugins.install_local").to_string())
                    .disabled(self.progress.is_some())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.manager_market = false;
                        this.choose_package(cx);
                    })),
            );
        let mut body = v_flex().gap_3().p_5();
        if let Some(at) = state
            .fetched_at
            .and_then(|seconds| chrono::DateTime::from_timestamp(i64::try_from(seconds).ok()?, 0))
        {
            body = body.child(
                t!(
                    "plugins.market_fetched_at",
                    time = at.format("%Y-%m-%d %H:%M:%S UTC").to_string()
                )
                .to_string(),
            );
        }
        if !state.fresh() {
            body = body.child(div().text_color(cx.theme().muted_foreground).child(
                if state.refreshing {
                    t!("plugins.market_refreshing").to_string()
                } else {
                    t!(
                        "plugins.market_offline",
                        error = state.error.as_deref().unwrap_or("")
                    )
                    .to_string()
                },
            ));
        }
        if let Some(status) = &self.status {
            body = body.child(
                div()
                    .text_color(cx.theme().danger)
                    .child(status.message.clone()),
            );
        }
        if let Some(listing) = selected {
            let platform = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
            let compatible = listing.compatible(env!("CARGO_PKG_VERSION"), &platform);
            let release = compatible.unwrap_or(&listing.versions[0]);
            let installed = self.entries.iter().any(|e| e.manifest.id == listing.id);
            let candidate = release.clone();
            let generation = state.generation();
            let source = format!("https://github.com/{}", listing.repository);
            let feedback = format!("{source}/issues");
            body = body
                .child(
                    div()
                        .text_xl()
                        .font_semibold()
                        .child(release.manifest.name.clone()),
                )
                .child(format!(
                    "{} · {} · {}",
                    release.version,
                    listing.maintainers.join(", "),
                    release.license
                ))
                .child(listing.summary.clone())
                .child(
                    t!(
                        "plugins.market_published_at",
                        time = release.published_at.clone()
                    )
                    .to_string(),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("market-source")
                                .debug_selector(|| "market-source".into())
                                .label(t!("plugins.market_source").to_string())
                                .on_click(move |_, _, cx| cx.open_url(&source)),
                        )
                        .child(
                            Button::new("market-feedback")
                                .debug_selector(|| "market-feedback".into())
                                .label(t!("plugins.market_feedback").to_string())
                                .on_click(move |_, _, cx| cx.open_url(&feedback)),
                        ),
                )
                .child(
                    t!(
                        "plugins.market_identity",
                        id = listing.id.clone(),
                        repository = listing.repository.clone()
                    )
                    .to_string(),
                )
                .child(
                    t!(
                        "plugins.market_compatibility",
                        host = release.host_version.clone(),
                        platforms = if release.platforms.is_empty() {
                            "*".into()
                        } else {
                            release.platforms.join(", ")
                        }
                    )
                    .to_string(),
                )
                .child(
                    t!(
                        "plugins.market_permissions",
                        permissions = release
                            .manifest
                            .permissions
                            .iter()
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                    .to_string(),
                )
                .child(listing.tags.join(" · "))
                .child(
                    listing
                        .downloads
                        .map(|count| t!("plugins.market_downloads", count = count).to_string())
                        .unwrap_or_else(|| t!("plugins.market_downloads_unavailable").to_string()),
                )
                .child(
                    Button::new("market-install")
                        .debug_selector(|| "market-install".into())
                        .label(if installed {
                            t!("plugins.installed").to_string()
                        } else {
                            t!("plugins.install").to_string()
                        })
                        .disabled(
                            installed
                                || !self.worker.state.lock().unwrap().ready
                                || compatible.is_none()
                                || !state.fresh()
                                || self.progress.is_some()
                                || !self
                                    .worker
                                    .trusted
                                    .load(std::sync::atomic::Ordering::Acquire),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.confirm_market_install(candidate.clone(), generation, window, cx);
                        })),
                );
            if compatible.is_none() {
                body = body.child(t!("plugins.market_incompatible").to_string());
            }
            if let Some(bitmap) = self
                .worker
                .state
                .lock()
                .unwrap()
                .market_icons
                .get(&release.sha256)
            {
                body = body.child(gpui_kit::img(bitmap.image.clone()).size(px(24.)));
            }
            body = body
                .child(ui::controls::untrusted_markdown_view(
                    "market-readme",
                    release.readme.clone(),
                    typography::font_size(cx),
                    cx,
                ))
                .child(
                    div()
                        .font_semibold()
                        .child(t!("plugins.changes").to_string()),
                )
                .child(ui::controls::untrusted_markdown_view(
                    "market-changelog",
                    release.changelog.clone(),
                    typography::font_size(cx),
                    cx,
                ));
        } else {
            body = body.child(t!("plugins.empty_market").to_string());
        }
        h_flex()
            .size_full()
            .child(sidebar)
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w(px(0.))
                    .h_full()
                    .child(
                        div()
                            .id("online-market-details")
                            .debug_selector(|| "online-market-details".into())
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
            )
            .into_any_element()
    }

    /// Show source and every requested permission before any package download can start.
    fn confirm_market_install(
        &mut self,
        release: Release,
        generation: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let owner = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let release = release.clone();
            let candidate = release.clone();
            let owner = owner.clone();
            dialog
                .title(t!("plugins.install").to_string())
                .width(px(540.))
                .overlay_closable(false)
                .content(move |content, _, _| {
                    content.child(
                        v_flex()
                            .gap_2()
                            .child(release.manifest.name.clone())
                            .child(t!("plugins.consent_unsigned").to_string())
                            .child(release.url.clone())
                            .child(
                                t!(
                                    "plugins.market_permissions",
                                    permissions = release
                                        .manifest
                                        .permissions
                                        .iter()
                                        .cloned()
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                )
                                .to_string(),
                            )
                            .child(
                                t!(
                                    "plugins.market_native_services",
                                    services = release
                                        .manifest
                                        .services
                                        .keys()
                                        .cloned()
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                )
                                .to_string(),
                            ),
                    )
                })
                .footer(
                    DialogFooter::new()
                        .child(
                            DialogClose::new()
                                .trigger(|b| b.label(t!("plugins.cancel").to_string())),
                        )
                        .child(
                            DialogAction::new().child(
                                Button::new("confirm-market-install")
                                    .debug_selector(|| "confirm-market-install".into())
                                    .label(t!("plugins.install").to_string())
                                    .primary(),
                            ),
                        ),
                )
                .on_ok(move |_, _, cx| {
                    owner
                        .update(cx, |this, cx| {
                            let accepted =
                                this.worker.install_market(candidate.clone(), generation);
                            cx.notify();
                            accepted
                        })
                        .unwrap_or(false)
                })
        });
    }
}

/// Cycling includes the unfiltered choice and remains operable with the local button keyboard behavior.
fn next_filter(current: &str, values: &std::collections::BTreeSet<String>) -> String {
    values
        .iter()
        .find(|value| value.as_str() > current)
        .cloned()
        .unwrap_or_default()
}
