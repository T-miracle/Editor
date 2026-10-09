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
    path: PathBuf,
    document: Option<DocumentVersion>,
    directory: bool,
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
        // Workspace and open-session paths are canonical; Windows display paths may omit the prefix.
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let document = self
            .tabs
            .iter()
            .position(|tab| tab.path() == path)
            .and_then(|index| self.plugin_document_version(index).ok());
        Target {
            path: path.clone(),
            document,
            directory: path.is_dir(),
        }
    }

    /// Conditions read native state. Descriptive paths are relative and confer no grants.
    pub(crate) fn plugin_menu_context(&self, target: &Target, cx: &App) -> MenuContext {
        let current = self.active_path.as_deref() == Some(target.path.as_path());
        let opened = self.tabs.iter().find(|tab| tab.path() == target.path);
        MenuContext {
            has_selection: current && !self.editor.read(cx).selected_range().is_empty(),
            writable: if current {
                self.editor.read(cx).is_editable()
            } else if let Some(tab) = opened {
                tab.text
                    .as_ref()
                    .is_some_and(|text| text.editor.read(cx).is_editable())
            } else {
                std::fs::metadata(&target.path)
                    .is_ok_and(|metadata| !metadata.permissions().readonly())
            },
            directory: target.directory,
            language: (!target.directory).then(|| crate::editor::language_for_path(&target.path)),
            extension: target
                .path
                .extension()
                .map(|extension| extension.to_string_lossy().to_lowercase()),
            path: target
                .path
                .strip_prefix(self.workspace.root())
                .ok()
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
        if let Some(document) = &target.document {
            // A captured nonactive tab retains its own session identity; focus is not authority.
            if !(0..self.tabs.len()).any(|index| {
                self.plugin_document_version(index)
                    .is_ok_and(|current| current == *document)
            }) {
                return;
            }
        } else if !target.path.exists() || target.path.is_dir() != target.directory {
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
            Ok(epoch) => self.reveal_menu_panel(&row.plugin, &row.command, epoch, window, cx),
        }
        cx.notify();
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
