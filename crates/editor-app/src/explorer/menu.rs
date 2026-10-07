//! Explorer commands built with GPUI Kit's PopupMenu, menu items and native submenus.

use crate::app::messages::MessageLevel;
use crate::*;
use gpui_kit::component::menu::{PopupMenu as KitPopupMenu, PopupMenuItem};
use gpui_kit::{ClipboardItem, DismissEvent, anchored};

const MENU_WIDTH: f32 = 212.;
const SUBMENU_WIDTH: f32 = 180.;
const SPECIAL_COPY_SUBMENU_WIDTH: f32 = 230.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Command {
    Copy,
    SpecialCopy,
    CopyFileName,
    CopyAbsolutePath,
    CopyProjectRoot,
    Paste,
    New,
    NewDirectory,
    NewFile,
    Delete,
    Rename,
    Refresh,
}

const ROW_COMMANDS: &[Command] = &[
    Command::Copy,
    Command::SpecialCopy,
    Command::Paste,
    Command::New,
    Command::Delete,
    Command::Rename,
    Command::Refresh,
];
const ROOT_COMMANDS: &[Command] = &[Command::Paste, Command::New, Command::Refresh];
const NEW_COMMANDS: &[Command] = &[Command::NewDirectory, Command::NewFile];
const SPECIAL_COPY_COMMANDS: &[Command] = &[
    Command::CopyFileName,
    Command::CopyAbsolutePath,
    Command::CopyProjectRoot,
];

impl Command {
    /// Only parent commands own a second menu; children remain executable actions.
    fn submenu_commands(self) -> Option<&'static [Command]> {
        match self {
            Self::New => Some(NEW_COMMANDS),
            Self::SpecialCopy => Some(SPECIAL_COPY_COMMANDS),
            _ => None,
        }
    }

    fn label(self) -> String {
        match self {
            Self::Copy => t!("explorer.copy"),
            Self::SpecialCopy => t!("explorer.special_copy"),
            Self::CopyFileName => t!("explorer.copy_file_name"),
            Self::CopyAbsolutePath => t!("explorer.copy_absolute_path"),
            Self::CopyProjectRoot => t!("explorer.copy_project_root"),
            Self::Paste => t!("explorer.paste"),
            Self::New => t!("explorer.new"),
            Self::NewDirectory => t!("explorer.directory"),
            Self::NewFile => t!("explorer.file"),
            Self::Delete => t!("explorer.delete"),
            Self::Rename => t!("explorer.rename"),
            Self::Refresh => t!("explorer.refresh"),
        }
        .to_string()
    }

    #[cfg(test)]
    fn id(self) -> &'static str {
        match self {
            Self::Copy => "explorer-menu-copy",
            Self::SpecialCopy => "explorer-menu-special-copy",
            Self::CopyFileName => "explorer-menu-copy-file-name",
            Self::CopyAbsolutePath => "explorer-menu-copy-absolute-path",
            Self::CopyProjectRoot => "explorer-menu-copy-project-root",
            Self::Paste => "explorer-menu-paste",
            Self::New => "explorer-menu-new",
            Self::NewDirectory => "explorer-menu-directory",
            Self::NewFile => "explorer-menu-file",
            Self::Delete => "explorer-menu-delete",
            Self::Rename => "explorer-menu-rename",
            Self::Refresh => "explorer-menu-refresh",
        }
    }
}

/// Retain the operation target and component lifetime; the native menu owns navigation and styling.
pub(crate) struct ExplorerMenu {
    target: Option<PathBuf>,
    folder: bool,
    position: Point<Pixels>,
    popup: Entity<KitPopupMenu>,
    _dismiss: Subscription,
}

/// Build documented menu items and submenus while dispatching file operations to their owner.
fn build_menu(
    mut menu: KitPopupMenu,
    commands: &'static [Command],
    owner: WeakEntity<EditorApp>,
    window: &mut Window,
    cx: &mut Context<KitPopupMenu>,
) -> KitPopupMenu {
    for command in commands.iter().copied() {
        if command == Command::Refresh {
            menu = menu.separator();
        }
        if let Some(children) = command.submenu_commands() {
            let child_owner = owner.clone();
            menu = menu.submenu(command.label(), window, cx, move |submenu, window, cx| {
                let width = if command == Command::SpecialCopy {
                    SPECIAL_COPY_SUBMENU_WIDTH
                } else {
                    SUBMENU_WIDTH
                };
                build_menu(
                    submenu.min_w(px(width)),
                    children,
                    child_owner.clone(),
                    window,
                    cx,
                )
            });
        } else {
            let action_owner = owner.clone();
            menu = menu.item(
                PopupMenuItem::new(command.label()).on_click(move |_, window, cx| {
                    let _ = action_owner.update(cx, |app, cx| {
                        app.run_explorer_menu_command(command, window, cx);
                    });
                }),
            );
        }
    }
    menu
}

impl EditorApp {
    /// Open the component at the pointer while keeping virtual tree rows independent of its lifetime.
    pub(crate) fn open_explorer_menu(
        &mut self,
        target: Option<PathBuf>,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The project root exposes workspace commands and cannot be renamed or deleted.
        let target = target.filter(|path| path != self.workspace.root());
        let folder = target.as_ref().is_none_or(|path| path.is_dir());
        let commands = if target.is_some() {
            ROW_COMMANDS
        } else {
            ROOT_COMMANDS
        };
        let owner = cx.entity().downgrade();
        let previous_focus = window
            .focused(cx)
            .unwrap_or_else(|| self.editor.focus_handle(cx));
        let popup = KitPopupMenu::build(window, cx, |menu, window, cx| {
            build_menu(
                menu.min_w(px(MENU_WIDTH)).action_context(previous_focus),
                commands,
                owner,
                window,
                cx,
            )
        });
        let popup_id = popup.entity_id();
        let dismiss = cx.subscribe(&popup, move |this, _, _: &DismissEvent, cx| {
            // A delayed dismissal from an older menu must not close a newly opened one.
            if this
                .explorer_menu
                .as_ref()
                .is_some_and(|menu| menu.popup.entity_id() == popup_id)
            {
                this.explorer_menu = None;
                cx.notify();
            }
        });
        popup.focus_handle(cx).focus(window, cx);
        self.plugin_popup = None;
        self.explorer_menu = Some(ExplorerMenu {
            target,
            folder,
            position,
            popup,
            _dismiss: dismiss,
        });
        cx.notify();
    }

    fn run_explorer_menu_command(
        &mut self,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = self.explorer_menu.take() else {
            return;
        };
        let target = menu
            .target
            .unwrap_or_else(|| self.workspace.root().to_path_buf());
        match command {
            Command::Copy => self.copy_explorer_path(&target, cx),
            Command::CopyFileName | Command::CopyAbsolutePath | Command::CopyProjectRoot => {
                // Tree targets and the workspace root are absolute; write text rather than CF_HDROP.
                if let Some(text) = special_copy_text(command, &target, self.workspace.root()) {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                    // The explicit clipboard result is retained without observing later status changes.
                    self.report_host_message(
                        MessageLevel::Info,
                        t!("explorer.text_copied").to_string(),
                        cx,
                    );
                }
            }
            Command::Paste => self.paste_explorer_path(&target, menu.folder, cx),
            Command::NewDirectory => self.start_explorer_edit(
                ExplorerEditKind::Directory,
                target,
                menu.folder,
                window,
                cx,
            ),
            Command::NewFile => {
                self.start_explorer_edit(ExplorerEditKind::File, target, menu.folder, window, cx)
            }
            Command::Delete => self.start_explorer_delete(target, window, cx),
            Command::Rename => {
                self.start_explorer_edit(ExplorerEditKind::Rename, target, menu.folder, window, cx)
            }
            Command::Refresh => {
                // Manual refresh also checks every open tab against its disk contents.
                self.host_refresh_pending = true;
                self.file_watch.reconcile();
                self.status = t!("status.refreshing_workspace").to_string();
            }
            Command::New | Command::SpecialCopy => unreachable!(),
        }
        cx.notify();
    }

    /// The host only positions the component and consumes outside presses; Kit draws both menu levels.
    pub(crate) fn render_explorer_menu(
        &self,
        _window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let Some(menu) = &self.explorer_menu else {
            return div().into_any_element();
        };
        div()
            .id("explorer-menu-overlay")
            .absolute()
            .inset_0()
            .occlude()
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_up(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.explorer_menu = None;
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _, _, cx| {
                    this.explorer_menu = None;
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .child(
                anchored()
                    .position(menu.position)
                    .snap_to_window_with_margin(px(8.))
                    .child(
                        div()
                            .id("explorer-context-menu")
                            .debug_selector(|| "explorer-context-menu".into())
                            .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
                            .child(menu.popup.clone()),
                    ),
            )
            .into_any_element()
    }
}

/// Copy the selected entry's name, disk path, or path relative to the workspace root.
fn special_copy_text(command: Command, target: &Path, project_root: &Path) -> Option<String> {
    match command {
        Command::CopyFileName => target
            .file_name()
            .map(|name| name.to_string_lossy().into_owned()),
        Command::CopyAbsolutePath => Some(clipboard_path_text(target)),
        Command::CopyProjectRoot => target.strip_prefix(project_root).ok().map(|relative| {
            // A root selection has no remaining components; represent the current directory as '.'.
            if relative.as_os_str().is_empty() {
                ".".to_owned()
            } else {
                clipboard_path_text(relative)
            }
        }),
        _ => None,
    }
}

/// Hide Windows verbatim prefixes from text paths while preserving valid UNC paths.
fn clipboard_path_text(path: &Path) -> String {
    let text = path.to_string_lossy();
    #[cfg(windows)]
    {
        if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
            return format!(r"\\{}", unc);
        }
        if let Some(local) = text.strip_prefix(r"\\?\") {
            if local.as_bytes().get(1) == Some(&b':') {
                return local.to_owned();
            }
        }
    }
    text.into_owned()
}

#[cfg(test)]
mod tests;
