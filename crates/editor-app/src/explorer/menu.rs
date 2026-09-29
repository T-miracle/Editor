//! Project-owned explorer menu appearance over GPUI's existing tree and pointer events.

use crate::ui::controls::menu::MenuStyle;
use crate::*;
use gpui_kit::KeyDownEvent;

const MENU_WIDTH: f32 = 212.;
const SUBMENU_WIDTH: f32 = 180.;
const ROW_HEIGHT: f32 = 29.;
const WINDOW_MARGIN: f32 = 8.;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Command {
    Copy,
    Paste,
    New,
    NewDirectory,
    NewFile,
    Rename,
    Refresh,
}

const ROW_COMMANDS: &[Command] = &[
    Command::Copy,
    Command::Paste,
    Command::New,
    Command::Rename,
    Command::Refresh,
];
const ROOT_COMMANDS: &[Command] = &[Command::Paste, Command::New, Command::Refresh];
const NEW_COMMANDS: &[Command] = &[Command::NewDirectory, Command::NewFile];

impl Command {
    fn label(self) -> String {
        match self {
            Self::Copy => t!("explorer.copy"),
            Self::Paste => t!("explorer.paste"),
            Self::New => t!("explorer.new"),
            Self::NewDirectory => t!("explorer.directory"),
            Self::NewFile => t!("explorer.file"),
            Self::Rename => t!("explorer.rename"),
            Self::Refresh => t!("explorer.refresh"),
        }
        .to_string()
    }

    fn id(self) -> &'static str {
        match self {
            Self::Copy => "explorer-menu-copy",
            Self::Paste => "explorer-menu-paste",
            Self::New => "explorer-menu-new",
            Self::NewDirectory => "explorer-menu-directory",
            Self::NewFile => "explorer-menu-file",
            Self::Rename => "explorer-menu-rename",
            Self::Refresh => "explorer-menu-refresh",
        }
    }
}

/// The menu stores only its target and navigation state; file actions stay on EditorApp.
pub(crate) struct ExplorerMenu {
    target: Option<PathBuf>,
    folder: bool,
    position: Point<Pixels>,
    focus: FocusHandle,
    focused_main: usize,
    focused_sub: usize,
    submenu_open: bool,
}

impl ExplorerMenu {
    fn commands(&self) -> &'static [Command] {
        if self.target.is_some() {
            ROW_COMMANDS
        } else {
            ROOT_COMMANDS
        }
    }

    fn selected(&self) -> Command {
        if self.submenu_open {
            NEW_COMMANDS[self.focused_sub]
        } else {
            self.commands()[self.focused_main]
        }
    }
}

impl EditorApp {
    pub(crate) fn open_explorer_menu(
        &mut self,
        target: Option<PathBuf>,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let folder = target.as_ref().is_none_or(|path| path.is_dir());
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        self.plugin_popup = None;
        self.explorer_menu = Some(ExplorerMenu {
            target,
            folder,
            position,
            focus,
            focused_main: 0,
            focused_sub: 0,
            submenu_open: false,
        });
        cx.notify();
    }

    fn select_explorer_menu_item(
        &mut self,
        command: Command,
        index: usize,
        submenu: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(menu) = &mut self.explorer_menu {
            if submenu {
                menu.focused_sub = index;
            } else {
                menu.focused_main = index;
                menu.submenu_open = command == Command::New;
            }
            cx.notify();
        }
    }

    fn run_explorer_menu_command(
        &mut self,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if command == Command::New {
            if let Some(menu) = &mut self.explorer_menu {
                menu.submenu_open = true;
                menu.focused_sub = 0;
                cx.notify();
            }
            return;
        }
        let Some(menu) = self.explorer_menu.take() else {
            return;
        };
        let target = menu
            .target
            .unwrap_or_else(|| self.workspace.root().to_path_buf());
        match command {
            Command::Copy => self.copy_explorer_path(&target, cx),
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
            Command::Rename => {
                self.start_explorer_edit(ExplorerEditKind::Rename, target, menu.folder, window, cx)
            }
            Command::Refresh => self.refresh_files(cx),
            Command::New => unreachable!(),
        }
        cx.notify();
    }

    fn explorer_menu_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = &mut self.explorer_menu else {
            return;
        };
        let key = event.keystroke.key.as_str();
        match key {
            "escape" => {
                cx.stop_propagation();
                if menu.submenu_open {
                    menu.submenu_open = false;
                } else {
                    self.explorer_menu = None;
                }
                cx.notify();
            }
            "up" | "down" => {
                cx.stop_propagation();
                let main_len = menu.commands().len();
                let (index, len) = if menu.submenu_open {
                    (&mut menu.focused_sub, NEW_COMMANDS.len())
                } else {
                    (&mut menu.focused_main, main_len)
                };
                *index = if key == "down" {
                    (*index + 1) % len
                } else {
                    (*index + len - 1) % len
                };
                cx.notify();
            }
            "right" if menu.selected() == Command::New => {
                cx.stop_propagation();
                menu.submenu_open = true;
                menu.focused_sub = 0;
                cx.notify();
            }
            "left" if menu.submenu_open => {
                cx.stop_propagation();
                menu.submenu_open = false;
                cx.notify();
            }
            "enter" => {
                cx.stop_propagation();
                let command = menu.selected();
                self.run_explorer_menu_command(command, window, cx);
            }
            _ => {}
        }
    }

    fn explorer_menu_row(
        &self,
        command: Command,
        index: usize,
        submenu: bool,
        selected: bool,
        style: &MenuStyle,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let view = cx.entity();
        style
            .row(command.id(), command.label(), selected)
            .debug_selector(move || command.id().into())
            .role(gpui_kit::Role::MenuItem)
            .accessibility_label(command.label())
            .focusable(false)
            .flex()
            .h(px(ROW_HEIGHT))
            .w_full()
            .items_center()
            .px(px(style.padding_x + 6.))
            .rounded(px((style.radius - 2.).max(2.)))
            .bg(if selected { style.hover } else { style.surface })
            .text_color(if selected {
                style.hover_foreground
            } else {
                style.foreground
            })
            .on_hover(move |hovered, _, cx| {
                if *hovered {
                    let _ = view.update(cx, |app, cx| {
                        app.select_explorer_menu_item(command, index, submenu, cx)
                    });
                }
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, window, cx| {
                this.run_explorer_menu_command(command, window, cx)
            }))
            .child(command.label())
            .child(div().flex_1())
            .when(command == Command::New, |this| {
                this.child(Icon::new(IconName::ChevronRight).xsmall())
            })
    }

    pub(crate) fn render_explorer_menu(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let Some(menu) = &self.explorer_menu else {
            return div().into_any_element();
        };
        let style = MenuStyle::current(cx);
        let main_height = menu.commands().len() as f32 * ROW_HEIGHT + style.padding_y * 2. + 9.;
        let (left, top) = menu_position(
            menu.position,
            window.viewport_size(),
            MENU_WIDTH,
            main_height,
        );
        let mut main_rows = Vec::new();
        for (index, command) in menu.commands().iter().copied().enumerate() {
            if command == Command::Refresh {
                main_rows.push(
                    div()
                        .h(px(9.))
                        .px_2()
                        .flex()
                        .items_center()
                        .child(div().h(px(1.)).w_full().bg(style.border))
                        .into_any_element(),
                );
            }
            main_rows.push(
                self.explorer_menu_row(
                    command,
                    index,
                    false,
                    !menu.submenu_open && menu.focused_main == index,
                    &style,
                    cx,
                )
                .into_any_element(),
            );
        }
        let main = style
            .card(MENU_WIDTH)
            .id("explorer-context-menu")
            .debug_selector(|| "explorer-context-menu".into())
            .role(gpui_kit::Role::Menu)
            .children(main_rows);
        let submenu = if menu.submenu_open {
            let sub_height = NEW_COMMANDS.len() as f32 * ROW_HEIGHT + style.padding_y * 2.;
            let new_index = menu
                .commands()
                .iter()
                .position(|command| *command == Command::New)
                .unwrap_or(0);
            let preferred_x = left + px(MENU_WIDTH - 2.);
            let sub_x = if preferred_x + px(SUBMENU_WIDTH + WINDOW_MARGIN)
                <= window.viewport_size().width
            {
                preferred_x
            } else {
                (left - px(SUBMENU_WIDTH - 2.)).max(px(WINDOW_MARGIN))
            };
            let sub_y = (top + px(new_index as f32 * ROW_HEIGHT + style.padding_y)).min(
                (window.viewport_size().height - px(sub_height + WINDOW_MARGIN))
                    .max(px(WINDOW_MARGIN)),
            );
            Some(
                div().absolute().left(sub_x).top(sub_y).child(
                    style
                        .card(SUBMENU_WIDTH)
                        .id("explorer-new-submenu")
                        .debug_selector(|| "explorer-new-submenu".into())
                        .role(gpui_kit::Role::Menu)
                        .children(
                            NEW_COMMANDS
                                .iter()
                                .copied()
                                .enumerate()
                                .map(|(index, command)| {
                                    self.explorer_menu_row(
                                        command,
                                        index,
                                        true,
                                        menu.focused_sub == index,
                                        &style,
                                        cx,
                                    )
                                    .into_any_element()
                                })
                                .collect::<Vec<_>>(),
                        ),
                ),
            )
        } else {
            None
        };
        div()
            .id("explorer-menu-overlay")
            .absolute()
            .inset_0()
            .track_focus(&menu.focus)
            .capture_key_down(cx.listener(Self::explorer_menu_key_down))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.explorer_menu = None;
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
            .child(div().absolute().left(left).top(top).child(main))
            .when_some(submenu, |this, submenu| this.child(submenu))
            .into_any_element()
    }
}

/// Clamp the menu to the window without changing the pointer target.
fn menu_position(
    position: Point<Pixels>,
    viewport: gpui_kit::Size<Pixels>,
    width: f32,
    height: f32,
) -> (Pixels, Pixels) {
    let left = position
        .x
        .min((viewport.width - px(width + WINDOW_MARGIN)).max(px(WINDOW_MARGIN)))
        .max(px(WINDOW_MARGIN));
    let top = position
        .y
        .min((viewport.height - px(height + WINDOW_MARGIN)).max(px(WINDOW_MARGIN)))
        .max(px(WINDOW_MARGIN));
    (left, top)
}

#[cfg(test)]
mod tests;
