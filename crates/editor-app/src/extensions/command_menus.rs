//! Native command contributions share live publication, target checks and deterministic grouping.
use super::*;
use crate::ui::controls::menu::MenuStyle;
use gpui_kit::component::menu::{PopupMenu as NativeMenu, PopupMenuItem};
use protocol::{
    api::DocumentVersion,
    commands::{Context as MenuContext, Location},
};

/// A rendered entry retains the exact contribution and instance epoch until native activation.
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
pub(crate) struct Contribution {
    pub(crate) plugin: String,
    pub(crate) command: String,
    pub(crate) label: String,
    pub(crate) disabled: bool,
    pub(crate) group: String,
    pub(crate) epoch: u64,
    menu: usize,
    order: i32,
}

/// An informational native target never becomes file authority or follows a newly active tab.
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
pub(crate) struct Target {
    // A virtual session has an exact document version but never a native filesystem path.
    path: Option<PathBuf>,
    document: Option<DocumentVersion>,
    directory: bool,
}

/// A comparison's native popup retains the same exact target and published command incarnations.
pub(crate) struct DocumentCommandMenu {
    target: Target,
    rows: Vec<Contribution>,
    popup: Entity<crate::ui::controls::menu::PopupMenu>,
    _dismiss: Subscription,
}

/// Selection contributions occur only with a real selection; all specified conditions intersect.
pub(crate) fn contributions(
    snapshot: &commands::ShortcutSnapshot,
    locations: &[Location],
    context: &MenuContext,
) -> Vec<Contribution> {
    let mut rows = Vec::new();
    if !snapshot.trusted || !snapshot.ready {
        return rows;
    }
    for entry in &snapshot.entries {
        // Application-scoped instances do not own this workspace's native file/selection context.
        if entry.manifest.scope == protocol::api::InstanceScope::Application {
            continue;
        }
        for command in &entry.manifest.commands {
            let Some(epoch) = snapshot
                .commands
                .get(&(entry.manifest.id.clone(), command.id.clone()))
            else {
                continue;
            };
            for (index, menu) in command.menus.iter().enumerate() {
                if !locations.contains(&menu.location)
                    || (menu.location == Location::Selection && !context.has_selection)
                    || !menu.when.matches(context)
                {
                    continue;
                }
                rows.push(Contribution {
                    plugin: entry.manifest.id.clone(),
                    command: command.id.clone(),
                    label: format!("{} · {}", command.title, entry.manifest.name),
                    disabled: !menu.enabled_when.matches(context),
                    group: menu.group.clone(),
                    epoch: *epoch,
                    menu: index,
                    order: menu.order,
                });
            }
        }
    }
    rows.sort_by(|left, right| {
        (
            &left.group,
            left.order,
            &left.plugin,
            &left.command,
            left.menu,
        )
            .cmp(&(
                &right.group,
                right.order,
                &right.plugin,
                &right.command,
                right.menu,
            ))
    });
    rows
}

impl EditorApp {
    /// Capture host document identity where available; directory menus retain their clicked path.
    pub(crate) fn plugin_menu_target(&self, path: &Path) -> Target {
        // Resolve virtual identities before any filesystem operation, including canonicalization.
        if let Some(index) = self
            .tabs
            .iter()
            .position(|tab| tab.virtual_document.is_some() && tab.path() == path)
        {
            return Target {
                path: None,
                document: self.plugin_document_version(index).ok(),
                directory: false,
            };
        }
        // Closed or unknown resource URIs remain non-filesystem targets; a missing tab
        // must not turn its former resource identity into a Windows path probe.
        if path.to_string_lossy().contains("://") {
            return Target {
                path: None,
                document: None,
                directory: false,
            };
        }
        // Workspace and open-session paths are canonical; Windows display paths may omit the prefix.
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let document = self
            .tabs
            .iter()
            .position(|tab| tab.path() == path)
            .and_then(|index| self.plugin_document_version(index).ok());
        Target {
            path: Some(path.clone()),
            document,
            directory: path.is_dir(),
        }
    }

    /// Conditions read native state. Descriptive paths are relative and confer no grants.
    pub(crate) fn plugin_menu_context(&self, target: &Target, cx: &App) -> MenuContext {
        let opened = self.tabs.iter().enumerate().find_map(|(index, tab)| {
            let matches = target.document.as_ref().map_or_else(
                || target.path.as_ref().is_some_and(|path| tab.path() == path),
                |document| {
                    self.plugin_document_version(index)
                        .is_ok_and(|live| live == *document)
                },
            );
            matches.then_some(tab)
        });
        let text = opened
            .and_then(|tab| tab.text.as_ref())
            .map(|tab| tab.editor.read(cx));
        let virtual_document = opened.and_then(|tab| tab.virtual_document.as_ref());
        MenuContext {
            // A focused comparison pane can be nonactive; selection belongs to its own entity.
            has_selection: text.is_some_and(|editor| !editor.selected_range().is_empty()),
            writable: if virtual_document.is_some() || target.path.is_none() {
                false
            } else if let Some(editor) = text {
                editor.is_editable()
            } else {
                target.path.as_ref().is_some_and(|path| {
                    std::fs::metadata(path).is_ok_and(|metadata| !metadata.permissions().readonly())
                })
            },
            directory: target.directory,
            language: virtual_document
                .map(|tab| tab.language.clone())
                .or_else(|| {
                    target
                        .path
                        .as_ref()
                        .filter(|_| !target.directory)
                        .map(|path| crate::editor::language_for_path(path))
                }),
            extension: target
                .path
                .as_ref()
                .and_then(|path| path.extension())
                .map(|extension| extension.to_string_lossy().to_lowercase()),
            path: target
                .path
                .as_ref()
                .and_then(|path| path.strip_prefix(self.workspace.root()).ok())
                .map(|path| path.to_string_lossy().replace('\\', "/")),
        }
    }

    /// Menu snapshots and shortcuts share one incarnation-aware command publication.
    pub(crate) fn plugin_menu_entries(
        &self,
        target: &Target,
        locations: &[Location],
        cx: &App,
    ) -> Vec<Contribution> {
        contributions(
            &self.extensions.read(cx).shortcut_snapshot(),
            locations,
            &self.plugin_menu_context(target, cx),
        )
    }

    /// Revalidate origin, revision, conditions and instance before queuing native activation.
    pub(crate) fn invoke_plugin_menu(
        &mut self,
        row: &Contribution,
        target: &Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.plugin_menu_target_is_current(target) {
            return;
        }
        let snapshot = self.extensions.read(cx).shortcut_snapshot();
        if snapshot
            .commands
            .get(&(row.plugin.clone(), row.command.clone()))
            != Some(&row.epoch)
        {
            return;
        }
        let Some(menu) = snapshot
            .entries
            .iter()
            .find(|entry| entry.manifest.id == row.plugin)
            .and_then(|entry| {
                entry
                    .manifest
                    .commands
                    .iter()
                    .find(|command| command.id == row.command)
            })
            .and_then(|command| command.menus.get(row.menu))
        else {
            return;
        };
        let context = self.plugin_menu_context(target, cx);
        if !menu.when.matches(&context)
            || !menu.enabled_when.matches(&context)
            || (menu.location == Location::Selection && !context.has_selection)
        {
            return;
        }
        match self.extensions.read(cx).enqueue_command(
            &row.plugin,
            &row.command,
            menu.arguments.clone(),
            Some(row.epoch),
            Some(context),
        ) {
            Err(error) => self.status = error,
            Ok(epoch) => {
                self.reveal_plugin_command_panel(&row.plugin, &row.command, epoch, window, cx)
            }
        }
        cx.notify();
    }

    /// Captured versions and virtual liveness are independent of the currently focused tab.
    fn plugin_menu_target_is_current(&self, target: &Target) -> bool {
        if let Some(document) = &target.document {
            // A captured nonactive tab retains its own session identity; focus is not authority.
            (0..self.tabs.len()).any(|index| {
                self.plugin_document_version(index)
                    .is_ok_and(|current| current == *document)
                    && self.tabs[index]
                        .virtual_document
                        .as_ref()
                        .is_none_or(|tab| tab.resource.is_live())
            })
        } else {
            target
                .path
                .as_ref()
                .is_some_and(|path| path.exists() && path.is_dir() == target.directory)
        }
    }

    /// Show readonly-pane commands with the existing local popup's focus, keyboard and appearance.
    pub(crate) fn open_plugin_document_menu(
        &mut self,
        target: Target,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_plugin_document_menu(window, cx);
        if !self.plugin_menu_target_is_current(&target) {
            return;
        }
        let rows = self.plugin_menu_entries(&target, &[Location::Editor, Location::Selection], cx);
        if rows.is_empty() {
            return;
        }
        let items = rows
            .iter()
            .enumerate()
            .map(|(index, row)| protocol::ui::MenuItem {
                id: format!("document-command-{index}"),
                label: row.label.clone(),
                disabled: row.disabled,
                separator_before: index > 0 && rows[index - 1].group != row.group,
            })
            .collect();
        let owner = cx.entity().downgrade();
        let action_rows = rows.clone();
        let action_target = target.clone();
        let popup = cx.new(|cx| {
            crate::ui::controls::menu::PopupMenu::new(
                items,
                MenuStyle::current(cx),
                position,
                move |action, window, cx| {
                    if let protocol::ui::Action::Select(id) = action
                        && let Some(index) = id
                            .strip_prefix("document-command-")
                            .and_then(|id| id.parse::<usize>().ok())
                        && let Some(row) = action_rows.get(index)
                    {
                        let _ = owner.update(cx, |owner, cx| {
                            owner.invoke_plugin_menu(row, &action_target, window, cx)
                        });
                    }
                },
                window,
                cx,
            )
        });
        let popup_id = popup.entity_id();
        let dismiss = cx.subscribe(&popup, move |owner, _, _: &gpui_kit::DismissEvent, cx| {
            // Late dismissal cannot dispose a new menu opened in the same native frame.
            if owner
                .document_command_menu
                .as_ref()
                .is_some_and(|menu| menu.popup.entity_id() == popup_id)
            {
                owner.document_command_menu = None;
                cx.notify();
            }
        });
        self.document_command_menu = Some(DocumentCommandMenu {
            target,
            rows,
            popup,
            _dismiss: dismiss,
        });
        cx.notify();
    }

    /// Dismiss before the origin comparison is removed, allowing its selective focus repair to run.
    pub(crate) fn close_plugin_document_menu(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(menu) = self.document_command_menu.take() {
            menu.popup.update(cx, |popup, cx| popup.dismiss(window, cx));
            cx.notify();
        }
    }

    /// Revoked versions or command incarnations must not leave an actionable captured popup.
    pub(crate) fn sync_plugin_document_menu(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.document_command_menu.as_ref().is_some_and(|menu| {
            !self.plugin_menu_target_is_current(&menu.target)
                || menu.rows.iter().any(|row| {
                    self.extensions
                        .read(cx)
                        .shortcut_epoch(&row.plugin, &row.command)
                        != Some(row.epoch)
                })
        }) {
            self.close_plugin_document_menu(window, cx);
        }
    }

    /// Refresh only local popup appearance; the existing entity owns navigation and scrolling.
    pub(crate) fn render_plugin_document_menu(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<Entity<crate::ui::controls::menu::PopupMenu>> {
        let menu = self.document_command_menu.as_ref()?;
        let style = MenuStyle::current(cx);
        menu.popup.update(cx, |popup, cx| {
            if popup.style != style {
                popup.style = style;
                cx.notify();
            }
        });
        Some(menu.popup.clone())
    }
}

/// GPUI's editor menu publishes ordinary typed actions; target metadata remains informational.
#[derive(gpui_kit::Action, Clone, PartialEq, serde::Deserialize)]
#[action(namespace = plugin, no_json)]
pub(crate) struct InvokeMenu {
    row: Contribution,
    target: Target,
}

/// The foundational editor menu keeps its native behavior while command actions retain their target.
pub(crate) fn append_editor(
    mut menu: gpui_kit::component::native_menu::NativeMenu,
    rows: Vec<Contribution>,
    target: Target,
) -> gpui_kit::component::native_menu::NativeMenu {
    let mut group = None;
    for row in rows {
        if group.as_ref() != Some(&row.group) {
            menu = menu.separator();
            group = Some(row.group.clone());
        }
        menu = menu.menu_with_disabled(
            row.label.clone(),
            row.disabled,
            Box::new(InvokeMenu {
                row,
                target: target.clone(),
            }),
        );
    }
    menu
}

impl EditorApp {
    /// Route a native editor menu action through the same fresh admission as explorer and tab menus.
    pub(crate) fn plugin_editor_menu_action(
        &mut self,
        action: &InvokeMenu,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.invoke_plugin_menu(&action.row, &action.target, window, cx);
    }
}

/// Extend an existing popup using Base menu behavior and the local product row appearance.
pub(crate) fn append(
    mut menu: NativeMenu,
    rows: Vec<Contribution>,
    target: Target,
    owner: WeakEntity<EditorApp>,
) -> NativeMenu {
    let mut group = None;
    for row in rows {
        if group.as_ref() != Some(&row.group) {
            menu = menu.separator();
            group = Some(row.group.clone());
        }
        let label = row.label.clone();
        let identity = format!("plugin-menu-{}/{}", row.plugin, row.command);
        let action_owner = owner.clone();
        let render_owner = owner.clone();
        let render_row = row.clone();
        let action_target = target.clone();
        menu = menu.item(
            PopupMenuItem::element(move |_, cx| {
                let live = render_owner.upgrade().is_some_and(|app| {
                    app.read(cx)
                        .extensions
                        .read(cx)
                        .shortcut_epoch(&render_row.plugin, &render_row.command)
                        == Some(render_row.epoch)
                });
                div().when(live, |body| {
                    body.child(
                        MenuStyle::current(cx)
                            .row(SharedString::from(identity.clone()), label.clone(), false)
                            .debug_selector({
                                let identity = identity.clone();
                                move || identity.clone()
                            })
                            .child(label.clone()),
                    )
                })
            })
            .disabled(row.disabled)
            .on_click(move |_, window, cx| {
                let _ = action_owner.update(cx, |app, cx| {
                    app.invoke_plugin_menu(&row, &action_target, window, cx)
                });
            }),
        );
    }
    menu
}
