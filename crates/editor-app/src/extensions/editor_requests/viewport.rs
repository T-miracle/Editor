//! Owned viewport locations reuse current document/scene authority without editing text or selection.
use super::*;
use protocol::api::ViewportTarget;

impl EditorApp {
    /// Queue a native locate only for the still-active, enabled split projection at this exact revision.
    pub(super) fn locate_plugin_viewport(
        &mut self,
        plugin: &str,
        request: EditorRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let result = (|| {
            let Op::LocateViewport {
                document,
                panel,
                ui_revision,
                target,
                origin,
            } = request.operation()
            else {
                unreachable!()
            };
            self.current_plugin_edit_target(document)?;
            target.validate()?;
            let owner = self
                .plugin_panels
                .get(&format!("{plugin}/{panel}"))
                .cloned()
                .ok_or_else(|| {
                    Failure::new(ErrorCode::NotFound, "Viewport owner is unavailable")
                })?;
            if self.active_editor_preview(cx).as_ref() != Some(&owner)
                || !self.editor_preview_sync_enabled(&owner, cx)
            {
                return Err(Failure::new(
                    ErrorCode::InvalidState,
                    "Synchronized split viewport is unavailable",
                ));
            }
            let projection = owner.read(cx);
            let scene = projection
                .current_document()
                .ok_or_else(|| Failure::new(ErrorCode::StaleRevision, "Viewport scene changed"))?;
            if scene.source.as_ref() != Some(document)
                || scene.revision != *ui_revision
                || scene.editor_viewport.is_none()
                || scene.dialog.is_some()
                || scene.menu.is_some()
            {
                return Err(Failure::new(
                    ErrorCode::StaleRevision,
                    "Viewport scene changed",
                ));
            }
            match target {
                ViewportTarget::Source {
                    offset,
                    line_fraction,
                } => {
                    let text = self.editor.read(cx).text();
                    if *offset > text.len() || !text.is_char_boundary(*offset) {
                        return Err(Failure::new(
                            ErrorCode::InvalidRequest,
                            "Source viewport requires a UTF-8 boundary",
                        ));
                    }
                    if !request.enter_side_effect() {
                        return Err(Failure::new(
                            ErrorCode::Cancelled,
                            "Viewport location cancelled",
                        ));
                    }
                    owner.update(cx, |panel, cx| {
                        panel.source_viewport.locate(
                            document.clone(),
                            *ui_revision,
                            *origin,
                            *offset,
                            *line_fraction,
                        )?;
                        cx.notify();
                        Ok::<_, Failure>(())
                    })?;
                    self.editor_panel.update(cx, |_, cx| cx.notify());
                    cx.notify();
                }
                ViewportTarget::Preview { node, fraction } => {
                    let block = scene
                        .active_node(node)
                        .and_then(|node| node.source_range)
                        .ok_or_else(|| {
                            Failure::new(
                                ErrorCode::InvalidState,
                                "Viewport target is not an active source block",
                            )
                        })?;
                    let text = self.editor.read(cx).text();
                    if block.end > text.len()
                        || !text.is_char_boundary(block.start)
                        || !text.is_char_boundary(block.end)
                    {
                        return Err(Failure::new(
                            ErrorCode::InvalidState,
                            "Viewport block no longer matches source",
                        ));
                    }
                    if !request.enter_side_effect() {
                        return Err(Failure::new(
                            ErrorCode::Cancelled,
                            "Viewport location cancelled",
                        ));
                    }
                    // An exact current publication may arrive before its first paint.
                    // Native preparation keeps the locate pending until ordinary geometry is available.
                    let native = owner.update(cx, |panel, cx| {
                        panel.native_document((*scene).clone(), window, cx)
                    });
                    native.update(cx, |view, cx| {
                        view.locate_viewport(node, *fraction, *origin, *ui_revision, cx)
                    })?;
                }
            }
            Ok(Value::Unit)
        })();
        request.finish(result);
    }
}
