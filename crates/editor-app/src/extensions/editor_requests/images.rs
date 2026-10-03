//! Image writes consume owned immutable gesture data, separately from any subsequent source transaction.
use super::*;
use gpui_kit::EntityInputHandler as _;

impl EditorApp {
    /// Validate the still-active version and selection before a background no-clobber file publication.
    /// A complete file remains even if its source subsequently changes; the receipt never edits text.
    pub(super) fn save_plugin_image(
        &mut self,
        request: EditorRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let result = (|| {
            let Op::SaveImageInput { input, name } = request.operation() else {
                unreachable!()
            };
            let image = request.image_input().ok_or_else(|| {
                Failure::new(ErrorCode::InvalidHandle, "Image resource is unavailable")
            })?;
            if image.input.handle != *input {
                return Err(Failure::new(
                    ErrorCode::InvalidHandle,
                    "Image owner mismatch",
                ));
            }
            let index = self.current_plugin_edit_target(&image.document)?;
            let editor = self.tabs[index].editor.clone();
            if editor.read(cx).selected_range() != (image.selection.start..image.selection.end) {
                return Err(Failure::new(
                    ErrorCode::StaleRevision,
                    "Selection changed before image save",
                ));
            }
            editor.update(cx, |editor, cx| {
                if !editor.is_editable() || editor.marked_text_range(window, cx).is_some() {
                    return Err(Failure::new(
                        ErrorCode::InvalidState,
                        "Editor is unavailable or composing text",
                    ));
                }
                Ok(())
            })?;
            let path = self.tabs[index].session.path().to_path_buf();
            self.check_plugin_save_path(&path)?;
            let root = self.workspace.root().to_path_buf();
            let (input, document, bytes, name) = (
                image.input.handle.clone(),
                image.document.clone(),
                image.bytes.clone(),
                name.clone(),
            );
            if !request.enter_side_effect() {
                return Err(Failure::new(
                    ErrorCode::Cancelled,
                    "Image save did not execute",
                ));
            }
            let completion = request.clone();
            cx.background_executor()
                .spawn(async move {
                    let saved =
                        platform_windows::create_document_attachment(&root, &path, &name, &bytes);
                    let result = saved
                        .map(|()| Value::ImageSaved {
                            input,
                            document,
                            name,
                        })
                        .map_err(|error| {
                            Failure::new(
                                match error.kind() {
                                    std::io::ErrorKind::AlreadyExists => ErrorCode::Conflict,
                                    std::io::ErrorKind::PermissionDenied => {
                                        ErrorCode::PermissionDenied
                                    }
                                    std::io::ErrorKind::InvalidInput => ErrorCode::InvalidPath,
                                    _ => ErrorCode::OperationFailed,
                                },
                                error.to_string(),
                            )
                        });
                    // Completion is thread-safe and wakes the normal manager polling path. No UI callback
                    // can redirect this result to the active document or claim a text insertion occurred.
                    completion.finish(result);
                })
                .detach();
            Ok(())
        })();
        if let Err(error) = result {
            request.finish(Err(error));
        }
    }
}
