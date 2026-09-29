//! Workspace file discovery, ordering, and tree construction.

// sort.rs remains unregistered until its collation rules replace the live tree ordering.
mod files;
pub(crate) mod menu;
pub(crate) mod tree;

use crate::ui::controls::Input;
use crate::*;
use gpui_base::input::InputState;

#[derive(Clone, Copy)]
pub(crate) enum ExplorerEditKind {
    Directory,
    File,
    Rename,
}

pub(crate) struct ExplorerEdit {
    kind: ExplorerEditKind,
    path: PathBuf,
    input: Entity<InputState>,
    error: Option<String>,
}

impl EditorApp {
    /// The context row is the destination for folders and its parent for files.
    pub(crate) fn start_explorer_edit(
        &mut self,
        kind: ExplorerEditKind,
        row: PathBuf,
        is_folder: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = if matches!(kind, ExplorerEditKind::Rename) || is_folder {
            row
        } else {
            row.parent().unwrap_or(self.workspace.root()).to_path_buf()
        };
        let initial = if matches!(kind, ExplorerEditKind::Rename) {
            path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        } else {
            String::new()
        };
        let input = cx.new(|cx| InputState::new(window, cx).default_value(initial));
        input.read(cx).focus_handle(cx).focus(window, cx);
        self.explorer_edit = Some(ExplorerEdit {
            kind,
            path,
            input,
            error: None,
        });
        cx.notify();
    }

    pub(crate) fn finish_explorer_edit(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.explorer_edit.as_ref() else {
            return;
        };
        let kind = edit.kind;
        let path = edit.path.clone();
        let name = edit.input.read(cx).value().to_string();
        let result = match kind {
            ExplorerEditKind::Directory => files::create(&path, &name, true),
            ExplorerEditKind::File => files::create(&path, &name, false),
            ExplorerEditKind::Rename => {
                if self
                    .tabs
                    .iter()
                    .any(|tab| tab.session.path().starts_with(&path))
                {
                    Err(t!("explorer.close_before_rename").to_string())
                } else {
                    files::rename(&path, &name)
                }
            }
        };
        match result {
            Ok(()) => {
                self.explorer_edit = None;
                self.refresh_files(cx);
            }
            Err(error) => {
                self.status = error.clone();
                if let Some(edit) = &mut self.explorer_edit {
                    edit.error = Some(error);
                }
                cx.notify();
            }
        }
    }

    pub(crate) fn copy_explorer_path(&mut self, path: &Path, cx: &mut Context<Self>) {
        match files::copy_to_clipboard(path) {
            Ok(()) => self.status = t!("explorer.copied").to_string(),
            Err(error) => self.status = error,
        }
        cx.notify();
    }

    pub(crate) fn paste_explorer_path(
        &mut self,
        row: &Path,
        is_folder: bool,
        cx: &mut Context<Self>,
    ) {
        let destination = if is_folder {
            row
        } else {
            row.parent().unwrap_or(self.workspace.root())
        };
        let sources = cx
            .read_from_clipboard()
            .map(|item| files::clipboard_paths(&item))
            .unwrap_or_default();
        match files::paste(&sources, destination) {
            Ok(()) => self.refresh_files(cx),
            Err(error) => {
                self.status = error;
                cx.notify();
            }
        }
    }

    pub(crate) fn render_explorer_edit(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(edit) = &self.explorer_edit else {
            return div().into_any_element();
        };
        let label = match edit.kind {
            ExplorerEditKind::Directory => t!("explorer.new_directory"),
            ExplorerEditKind::File => t!("explorer.new_file"),
            ExplorerEditKind::Rename => t!("explorer.rename"),
        };
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui_kit::rgba(0x00000066))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.stop_propagation()),
            )
            .child(
                v_flex()
                    .w(px(340.))
                    .gap_2()
                    .p_4()
                    .rounded_md()
                    .bg(cx.theme().popover)
                    .border_1()
                    .border_color(cx.theme().border)
                    .capture_key_down(cx.listener(|this, event: &gpui_kit::KeyDownEvent, _, cx| {
                        if event.keystroke.key == "enter" {
                            cx.stop_propagation();
                            this.finish_explorer_edit(cx);
                        } else if event.keystroke.key == "escape" {
                            cx.stop_propagation();
                            this.explorer_edit = None;
                            cx.notify();
                        }
                    }))
                    .child(label.to_string())
                    .child(Input::new(&edit.input))
                    .when_some(edit.error.clone(), |this, error| {
                        this.child(div().text_color(cx.theme().danger_foreground).child(error))
                    })
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("explorer-edit-ok")
                                    .label(t!("explorer.confirm").to_string())
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.finish_explorer_edit(cx)),
                                    ),
                            )
                            .child(
                                Button::new("explorer-edit-cancel")
                                    .label(t!("explorer.cancel").to_string())
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.explorer_edit = None;
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }
}
