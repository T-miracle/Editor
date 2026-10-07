//! Workspace file discovery, ordering, and tree construction.

// sort.rs remains unregistered until its collation rules replace the live tree ordering.
pub(crate) mod drag;
mod files;
mod interaction;
pub(crate) mod menu;
pub(crate) mod transfer;
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

/// Holds the reviewed target and modal focus until a delete is confirmed or cancelled.
pub(crate) struct ExplorerDelete {
    path: PathBuf,
    directory: bool,
    focus: FocusHandle,
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
                if self.tabs.iter().any(|tab| tab.path().starts_with(&path)) {
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
        match transfer::clipboard::write(&[path.to_path_buf()], transfer::Kind::Copy, cx) {
            Ok(()) => self.status = t!("explorer.copied").to_string(),
            Err(error) => self.status = error,
        }
        cx.notify();
    }

    pub(crate) fn paste_explorer_path(
        &mut self,
        row: &Path,
        is_folder: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let _ = is_folder;
        self.paste_file_offer(Some(row), window, cx);
    }

    pub(crate) fn start_explorer_delete(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tabs.iter().any(|tab| tab.path().starts_with(&path)) {
            self.status = t!("explorer.close_before_delete").to_string();
            cx.notify();
            return;
        }
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        self.explorer_delete = Some(ExplorerDelete {
            directory: path.is_dir(),
            path,
            focus,
            error: None,
        });
        cx.notify();
    }

    pub(crate) fn finish_explorer_delete(&mut self, cx: &mut Context<Self>) {
        let Some(delete) = &self.explorer_delete else {
            return;
        };
        let path = delete.path.clone();
        // A tab may have opened while the confirmation was visible; preserve its editor buffer.
        if self.tabs.iter().any(|tab| tab.path().starts_with(&path)) {
            self.status = t!("explorer.close_before_delete").to_string();
            self.explorer_delete = None;
            cx.notify();
            return;
        }
        match files::delete(self.workspace.root(), &path) {
            Ok(()) => {
                self.explorer_delete = None;
                self.refresh_files(cx);
            }
            Err(error) => {
                let message = t!("explorer.delete_failed", error = error).to_string();
                self.status = message.clone();
                if let Some(delete) = &mut self.explorer_delete {
                    delete.error = Some(message);
                }
                cx.notify();
            }
        }
    }

    pub(crate) fn render_explorer_delete(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(delete) = &self.explorer_delete else {
            return div().into_any_element();
        };
        let preview = if delete.directory {
            t!("explorer.delete_directory_preview")
        } else {
            t!("explorer.delete_file_preview")
        };
        div()
            .id("explorer-delete-preview")
            .debug_selector(|| "explorer-delete-preview".into())
            .absolute()
            .inset_0()
            // Keep hover and selection events behind the confirmation mask.
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui_kit::rgba(0x00000066))
            .track_focus(&delete.focus)
            .capture_key_down(cx.listener(|this, event: &gpui_kit::KeyDownEvent, _, cx| {
                match event.keystroke.key.as_str() {
                    "enter" => {
                        cx.stop_propagation();
                        this.finish_explorer_delete(cx);
                    }
                    "escape" => {
                        cx.stop_propagation();
                        this.explorer_delete = None;
                        cx.notify();
                    }
                    _ => {}
                }
            }))
            .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_up(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .child(
                v_flex()
                    .w(px(390.))
                    .gap_3()
                    .p_4()
                    .rounded_md()
                    .bg(cx.theme().popover)
                    .border_1()
                    .border_color(cx.theme().border)
                    .child(preview.to_string())
                    // Show the full target path before the irreversible filesystem operation.
                    .child(
                        div()
                            .text_color(cx.theme().muted_foreground)
                            .child(delete.path.display().to_string()),
                    )
                    .when_some(delete.error.clone(), |this, error| {
                        this.child(div().text_color(cx.theme().danger_foreground).child(error))
                    })
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                div()
                                    .id("explorer-delete-confirm")
                                    .debug_selector(|| "explorer-delete-confirm".into())
                                    .child(
                                        Button::new("explorer-delete-confirm-button")
                                            .label(t!("explorer.delete").to_string())
                                            .danger()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.finish_explorer_delete(cx)
                                            })),
                                    ),
                            )
                            .child(
                                div()
                                    .id("explorer-delete-cancel")
                                    .debug_selector(|| "explorer-delete-cancel".into())
                                    .child(
                                        Button::new("explorer-delete-cancel-button")
                                            .label(t!("explorer.cancel").to_string())
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.explorer_delete = None;
                                                cx.notify();
                                            })),
                                    ),
                            ),
                    ),
            )
            .into_any_element()
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
            // The modal backdrop must own hit testing over the editor popovers.
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui_kit::rgba(0x00000066))
            .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_up(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
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
                                div()
                                    .debug_selector(|| "explorer-edit-cancel".into())
                                    .child(
                                        Button::new("explorer-edit-cancel")
                                            .label(t!("explorer.cancel").to_string())
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.explorer_edit = None;
                                                cx.notify();
                                            })),
                                    ),
                            ),
                    ),
            )
            .into_any_element()
    }
}
