//! Native navigation uses the existing document-open path after source and workspace authority checks.
use super::*;
use protocol::api::NavigationTarget;

impl EditorApp {
    /// Recheck the originating document before selecting a generic navigation effect.
    pub(super) fn navigate_plugin_document(
        &mut self,
        plugin: &str,
        request: EditorRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Op::NavigateDocument { document, target } = request.operation() else {
            unreachable!()
        };
        let result = (|| {
            self.current_plugin_edit_target(document)?;
            target.validate()?;
            match target {
                NavigationTarget::RelativeDocument { path } => {
                    self.open_plugin_link(document, path, &request, window, cx)
                }
                NavigationTarget::ExternalUrl { url } => open_web_link(url, &request, cx),
                NavigationTarget::PreviewNode {
                    panel,
                    node,
                    ui_revision,
                } => self.reveal_plugin_link(
                    plugin,
                    document,
                    panel,
                    node,
                    *ui_revision,
                    &request,
                    window,
                    cx,
                ),
            }
        })();
        request.finish(result);
    }

    /// Resolve once against the source directory and the physical workspace, then use ordinary open.
    fn open_plugin_link(
        &mut self,
        document: &DocumentVersion,
        path: &str,
        request: &EditorRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Value, Failure> {
        let path = api::document_relative_path(path)?;
        let source = self.workspace.root().join(&document.path);
        let directory = source
            .parent()
            .ok_or_else(|| Failure::new(ErrorCode::InvalidPath, "Source has no directory"))?
            .canonicalize()
            .map_err(navigation_io_error)?;
        let target = directory
            .join(path)
            .canonicalize()
            .map_err(navigation_io_error)?;
        if !directory.starts_with(self.workspace.root())
            || !target.starts_with(self.workspace.root())
            || !target.is_file()
        {
            return Err(Failure::new(
                ErrorCode::InvalidPath,
                "Navigation target escaped its workspace or is not a document",
            ));
        }
        enter_navigation(request, "Document navigation cancelled")?;
        self.open_file(target.clone(), window, cx);
        let index = self
            .active_tab_index()
            .filter(|index| self.tabs[*index].path() == target)
            .ok_or_else(|| {
                Failure::new(ErrorCode::OperationFailed, "Document could not be opened")
            })?;
        Ok(Value::Opened {
            document: self.plugin_document_version(index)?,
        })
    }

    /// Only the current owned scene may reveal a real source block; no text or undo state is changed.
    #[allow(clippy::too_many_arguments)]
    fn reveal_plugin_link(
        &mut self,
        plugin: &str,
        document: &DocumentVersion,
        panel: &str,
        node: &str,
        ui_revision: u64,
        request: &EditorRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Value, Failure> {
        let key = format!("{plugin}/{panel}");
        let owner =
            self.plugin_panels.get(&key).cloned().ok_or_else(|| {
                Failure::new(ErrorCode::NotFound, "Owned preview is not available")
            })?;
        if self.active_editor_preview(cx).as_ref() != Some(&owner) {
            return Err(Failure::new(
                ErrorCode::InvalidState,
                "Preview is not visible",
            ));
        }
        let projection = owner.read(cx);
        let scene = projection
            .current_document()
            .ok_or_else(|| Failure::new(ErrorCode::StaleRevision, "Preview source changed"))?;
        if !projection.editor_preview
            || !projection.visible.get()
            || scene.source.as_ref() != Some(document)
            || scene.revision != ui_revision
        {
            return Err(Failure::new(
                ErrorCode::StaleRevision,
                "Preview scene changed",
            ));
        }
        let range = scene
            .active_node(node)
            .and_then(|node| node.source_range)
            .ok_or_else(|| {
                Failure::new(
                    ErrorCode::InvalidState,
                    "Target is not an active source block",
                )
            })?;
        let text = self.editor.read(cx).text().to_string();
        if text.get(range.start..range.end).is_none() {
            return Err(Failure::new(
                ErrorCode::InvalidState,
                "Block range no longer matches source",
            ));
        }
        enter_navigation(request, "Preview navigation cancelled")?;
        // Publication can precede the first paint after a file switch. Prepare this exact
        // authorized scene now; its normal next layout measures and executes the queued reveal.
        let native = owner.update(cx, |panel, cx| {
            panel.native_document((*scene).clone(), window, cx)
        });
        native.update(cx, |view, cx| view.reveal_node(node, ui_revision, cx))?;
        Ok(Value::Unit)
    }
}

/// Parse without launching, rejecting credentials and non-web schemes before the OS browser boundary.
fn open_web_link(
    url: &str,
    request: &EditorRequest,
    cx: &mut Context<EditorApp>,
) -> Result<Value, Failure> {
    let parsed = url::Url::parse(url)
        .map_err(|_| Failure::new(ErrorCode::InvalidPath, "Invalid browser URL"))?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(Failure::new(
            ErrorCode::InvalidPath,
            "Browser URL has no safe web authority",
        ));
    }
    enter_navigation(request, "Browser navigation cancelled")?;
    cx.open_url(parsed.as_str());
    Ok(Value::Unit)
}

/// The irreversible effect frontier shares the existing request cancellation and timeout ownership.
fn enter_navigation(request: &EditorRequest, message: &str) -> Result<(), Failure> {
    if request.enter_side_effect() {
        Ok(())
    } else {
        Err(Failure::new(ErrorCode::Cancelled, message))
    }
}

/// Keep missing files distinguishable from unreadable paths without opening another document on failure.
fn navigation_io_error(error: std::io::Error) -> Failure {
    Failure::new(
        match error.kind() {
            std::io::ErrorKind::NotFound => ErrorCode::NotFound,
            std::io::ErrorKind::PermissionDenied => ErrorCode::PermissionDenied,
            _ => ErrorCode::InvalidPath,
        },
        error.to_string(),
    )
}
