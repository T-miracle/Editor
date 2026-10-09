//! Native pickers consume validated requests and actively close with their original owner.
use super::*;
use platform_windows::file_picker::{FilePickerKind, FilePickerOptions, pick_files};
use plugin_runtime::plugin_protocol::interaction::{Operation, SelectionMode};
use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};

impl EditorApp {
    /// The controller belongs to this window; no guest path or HWND can redirect its selection.
    pub(super) fn select_plugin_resource(
        &mut self,
        plugin: &str,
        request: EditorRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Op::Interaction {
            operation:
                Operation::Select {
                    title,
                    mode,
                    multiple,
                    suggested_name,
                },
        } = request.operation()
        else {
            unreachable!()
        };
        let owner_hwnd = window
            .window_handle()
            .ok()
            .and_then(|handle| match handle.as_raw() {
                RawWindowHandle::Win32(window) => Some(window.hwnd.get()),
                _ => None,
            })
            .unwrap_or(0);
        let picker = pick_files(FilePickerOptions {
            owner_hwnd,
            title: format!("{title} · {plugin}"),
            multiple: *multiple,
            suggested_name: suggested_name.clone(),
            kind: match mode {
                SelectionMode::File => FilePickerKind::OpenFile,
                SelectionMode::Directory => FilePickerKind::Directory,
                SelectionMode::Save => FilePickerKind::Save,
            },
        });
        let picker = match picker {
            Ok(picker) => picker,
            Err(error) => {
                request.finish(Err(picker_failure(error)));
                return;
            }
        };
        self.plugin_picker_pending = true;
        self.plugin_file_picker = Some(picker.control);
        // try_recv never blocks the GPUI thread. Retirement and caller cancellation close the actual
        // system modal before its asynchronous result is considered for any resource grant.
        cx.spawn(async move |app, cx| {
            loop {
                if request.status().is_terminal() {
                    let closed = app.update(cx, |app, _| {
                        if let Some(control) = &app.plugin_file_picker {
                            control.cancel();
                        }
                    });
                    if closed.is_err() {
                        break;
                    }
                }
                let result = match picker.result.try_recv() {
                    Ok(result) => Some(result),
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(Err(
                        std::io::Error::other("Native picker ended without a result"),
                    )),
                    Err(std::sync::mpsc::TryRecvError::Empty) => None,
                };
                if let Some(result) = result {
                    match result {
                        Ok(Some(paths)) => request.finish_selection(paths),
                        Ok(None) => request.cancel_from_host(api::CancelMode::TryTerminate),
                        Err(error) => request.finish(Err(picker_failure(error))),
                    }
                    let _ = app.update(cx, |app, cx| {
                        app.plugin_file_picker = None;
                        app.plugin_picker_pending = false;
                        cx.notify();
                    });
                    break;
                }
                if app.update(cx, |_, _| ()).is_err() {
                    request.cancel_from_host(api::CancelMode::TryTerminate);
                    break;
                }
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(50))
                    .await;
            }
        })
        .detach();
    }
}

/// Preserve an explicit terminal failure when display or quota admission cannot complete.
fn picker_failure(error: std::io::Error) -> Failure {
    Failure::new(
        match error.kind() {
            std::io::ErrorKind::Unsupported => ErrorCode::UnsupportedOperation,
            std::io::ErrorKind::WouldBlock => ErrorCode::LimitExceeded,
            std::io::ErrorKind::InvalidInput => ErrorCode::InvalidRequest,
            _ => ErrorCode::OperationFailed,
        },
        error.to_string(),
    )
}
