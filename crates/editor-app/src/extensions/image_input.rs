//! Native user gestures offer bounded immutable image resources to one authorized editor surface.
use super::*;
use gpui_kit::component::WindowExt as _;
use gpui_kit::{ClipboardEntry, DragMoveEvent, ExternalPaths, PromptLevel};
use plugin_runtime::{HostImageInput, HostImageOrigin};
use protocol::api::{DocumentVersion, ErrorCode, Failure, TextRange};
use std::io::Read;

/// A capture cannot follow a new tab, revision, selection or replacement plugin instance.
struct Capture {
    plugin: String,
    panel: String,
    epoch: u64,
    document: DocumentVersion,
    selection: TextRange,
}

/// Only bytes or paths explicitly offered by this user gesture enter the background preparation.
enum Offered {
    Bytes(Arc<Vec<u8>>),
    File(PathBuf),
}

impl EditorApp {
    /// Native window capture keeps external drag release independent of the editor's opaque hitboxes.
    /// The callbacks retain only a weak app handle; the source surface owns their frame lifetime.
    pub(crate) fn render_image_drag_events(&self, cx: &Context<Self>) -> impl IntoElement {
        let app = cx.entity().downgrade();
        gpui_kit::canvas(
            |_, _, _| (),
            move |_, (), window, _| {
                let clearing = app.clone();
                window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                    if phase.capture() {
                        let _ =
                            clearing.update(cx, |app, cx| app.image_drag_clear(event, window, cx));
                    }
                });
                let releasing = app.clone();
                window.on_mouse_event(move |event: &gpui_kit::MouseUpEvent, phase, window, cx| {
                    if phase.capture() {
                        let _ = releasing
                            .update(cx, |app, cx| app.image_drag_release(event, window, cx));
                    }
                });
            },
        )
        .absolute()
        .size(px(0.))
    }

    /// Capture the same Paste action used by both keyboard shortcuts and the editor context menu.
    /// Text-only items keep the base editor's ordinary clipboard behavior.
    pub(crate) fn paste_plugin_images(
        &mut self,
        _: &gpui_base::input::Paste,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.editor.focus_handle(cx).is_focused(window) {
            return;
        }
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };
        let relevant = item.entries().iter().any(|entry| match entry {
            ClipboardEntry::Image(_) => true,
            ClipboardEntry::ExternalPaths(paths) => {
                paths.paths().iter().any(|path| image_path(path))
            }
            _ => false,
        });
        if !relevant {
            return;
        }
        let capture = match self.image_capture(true, window, cx) {
            Ok(Some(capture)) => capture,
            Ok(None) => return,
            Err(error) => {
                cx.stop_propagation();
                self.image_input_error(error.code, cx);
                return;
            }
        };
        cx.stop_propagation();
        if self.image_input_preparing {
            self.image_input_busy(cx);
            return;
        }
        let Some(reservation) = self.extensions.read(cx).worker.reserve_image_offer() else {
            self.image_input_error(ErrorCode::LimitExceeded, cx);
            return;
        };
        let mut offered = Vec::new();
        let mut total = 0usize;
        for entry in item.entries() {
            match entry {
                ClipboardEntry::Image(image) => {
                    total = total.saturating_add(image.bytes().len());
                    if offered.len() >= 8
                        || image.bytes().len() > 8 * 1024 * 1024
                        || total > 32 * 1024 * 1024
                    {
                        self.image_input_error(ErrorCode::LimitExceeded, cx);
                        cx.stop_propagation();
                        return;
                    }
                    offered.push(Offered::Bytes(Arc::new(image.bytes().to_vec())));
                }
                ClipboardEntry::ExternalPaths(paths) => {
                    for path in paths.paths().iter().filter(|path| image_path(path)) {
                        if offered.len() >= 8 {
                            self.image_input_error(ErrorCode::LimitExceeded, cx);
                            return;
                        }
                        offered.push(Offered::File(path.clone()));
                    }
                }
                ClipboardEntry::String(_) => {}
            }
        }
        if offered.is_empty() {
            return;
        }
        self.offer_images(
            capture,
            offered,
            reservation,
            HostImageOrigin::Clipboard,
            window,
            cx,
        );
    }

    /// Retain at most one bounded typed platform payload until its matching mouse release.
    /// Typed drag movement is a public capture hook and remains available under child hitboxes.
    pub(crate) fn image_drag_move(
        &mut self,
        event: &DragMoveEvent<ExternalPaths>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let paths = event
            .drag(cx)
            .paths()
            .iter()
            .filter(|path| image_path(path))
            .take(9)
            .cloned()
            .collect();
        self.image_drag = Some((ExternalPaths(paths), event.bounds));
    }

    /// A new internal pointer gesture cannot reuse paths from an earlier external drag.
    pub(crate) fn image_drag_clear(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
        self.image_drag = None;
    }

    /// Capture a typed external release before child hitboxes consume its bubble phase.
    /// Outside-source releases and non-opted-in editors keep the existing platform drop route.
    pub(crate) fn image_drag_release(
        &mut self,
        event: &gpui_kit::MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((paths, source_bounds)) = self.image_drag.take() else {
            return;
        };
        if event.button != MouseButton::Left
            || !source_bounds.contains(&event.position)
            || !cx.has_active_drag()
            || paths.paths().is_empty()
            || matches!(self.image_capture(false, window, cx), Ok(None))
        {
            return;
        }
        self.drop_plugin_images(&paths, window, cx);
        cx.stop_active_drag(window);
        cx.stop_propagation();
    }

    /// The external drop payload supplies only user-selected paths; it does not grant arbitrary guest reads.
    pub(crate) fn drop_plugin_images(
        &mut self,
        paths: &ExternalPaths,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let offered: Vec<_> = paths
            .paths()
            .iter()
            .filter(|path| image_path(path))
            .take(9)
            .cloned()
            .map(Offered::File)
            .collect();
        if offered.is_empty() {
            return;
        }
        // A busy drop cannot move the caret and invalidate the capture of the first accepted gesture.
        if self.image_input_preparing {
            self.image_input_busy(cx);
            return;
        }
        match self.image_capture(false, window, cx) {
            Ok(Some(_)) => {
                let Some(caret) = crate::editor::caret_offset_at(
                    self.editor.read(cx),
                    window.mouse_position(),
                    window,
                    cx,
                ) else {
                    return;
                };
                cx.stop_propagation();
                self.editor
                    .update(cx, |editor, cx| editor.set_selected_range(caret..caret, cx));
                // The chosen drop caret, rather than the previous keyboard selection, owns this intent.
                let Some(capture) = self.image_capture(false, window, cx).ok().flatten() else {
                    return;
                };
                let Some(reservation) = self.extensions.read(cx).worker.reserve_image_offer()
                else {
                    self.image_input_error(ErrorCode::LimitExceeded, cx);
                    return;
                };
                self.offer_images(
                    capture,
                    offered,
                    reservation,
                    HostImageOrigin::Drop,
                    window,
                    cx,
                );
            }
            Ok(None) => {}
            Err(error) => self.image_input_error(error.code, cx),
        }
    }

    /// Opt-in comes from a current public UI declaration, while installation grants remain authoritative.
    fn image_capture(
        &self,
        clipboard: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Option<Capture>, Failure> {
        // Window capture precedes overlay bubble blockers. Recheck host-owned modal/menu state
        // here so an external gesture cannot edit the document behind a blocking surface.
        if self.plugin_popup.is_some()
            || self.explorer_menu.is_some()
            || self.explorer_edit.is_some()
            || self.explorer_delete.is_some()
            || window.has_active_dialog(cx)
            || window.has_active_sheet(cx)
        {
            return Ok(None);
        }
        let Some(panel) = self.active_editor_preview(cx) else {
            return Ok(None);
        };
        if self.editor_preview_mode(&panel, cx) == protocol::PreviewMode::Preview {
            return Ok(None);
        }
        let (plugin, surface, epoch, document) = {
            let panel = panel.read(cx);
            let Some(tree) = panel
                .current_document()
                .filter(|tree| tree.editor_image_input)
            else {
                return Ok(None);
            };
            if tree.dialog.is_some() || tree.menu.is_some() {
                return Ok(None);
            }
            let plugin = panel
                .active
                .as_ref()
                .ok_or_else(|| Failure::new(ErrorCode::InvalidHandle, "Surface is unavailable"))?;
            let entry = panel
                .entries
                .iter()
                .find(|entry| &entry.manifest.id == plugin && entry.enabled)
                .ok_or_else(|| {
                    Failure::new(ErrorCode::PermissionDenied, "Plugin is unavailable")
                })?;
            if !self.session_state.workspace_trusted
                || !["editor.read", "editor.write", "workspace.write"]
                    .iter()
                    .all(|permission| entry.grants.contains(*permission))
                || (clipboard && !entry.grants.contains("clipboard"))
            {
                return Err(Failure::new(
                    ErrorCode::PermissionDenied,
                    "Image input permission is missing",
                ));
            }
            let document = tree
                .source
                .clone()
                .ok_or_else(|| Failure::new(ErrorCode::InvalidState, "No source document"))?;
            (
                plugin.clone(),
                panel.surface_id.clone().unwrap_or_default(),
                panel.instance_epoch,
                document,
            )
        };
        let protocol::api::EditorValue::DocumentSelection { range, .. } =
            self.read_plugin_document_selection(&document, window, cx)?
        else {
            unreachable!()
        };
        Ok(Some(Capture {
            plugin,
            panel: surface,
            epoch,
            document,
            selection: range,
        }))
    }

    /// Reject late clipboard reads, disk preparation and save prompts without following active editor state.
    fn capture_current(
        &self,
        capture: &Capture,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.image_capture(false, window, cx).is_ok_and(|now| {
            now.is_some_and(|now| {
                now.plugin == capture.plugin
                    && now.panel == capture.panel
                    && now.epoch == capture.epoch
                    && now.document == capture.document
                    && now.selection == capture.selection
            })
        })
    }

    /// A file-backed document whose path has disappeared must be saved before any image bytes land.
    fn offer_images(
        &mut self,
        capture: Capture,
        offered: Vec<Offered>,
        reservation: worker::ImageOfferReservation,
        origin: HostImageOrigin,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.image_input_preparing {
            self.image_input_busy(cx);
            return;
        }
        if offered.len() > 8 {
            self.image_input_error(ErrorCode::LimitExceeded, cx);
            return;
        }
        let path = self.workspace.root().join(&capture.document.path);
        if !path.is_file() {
            self.image_input_preparing = true;
            let save = t!("editor.image_save").to_string();
            let cancel = t!("editor.image_cancel").to_string();
            let answer = window.prompt(
                PromptLevel::Info,
                &t!("editor.image_save_first"),
                Some(&t!("editor.image_save_first_detail")),
                &[save.as_str(), cancel.as_str()],
                cx,
            );
            cx.spawn_in(window, async move |app, cx| {
                let save = answer.await == Ok(0);
                let _ = app.update_in(cx, |app, window, cx| {
                    app.image_input_preparing = false;
                    if !save {
                        return;
                    }
                    if !app.capture_current(&capture, window, cx) {
                        return;
                    }
                    if app.save_image_source(&capture, cx).is_err() {
                        app.image_input_error(ErrorCode::OperationFailed, cx);
                        return;
                    }
                    if path.is_file() && app.capture_current(&capture, window, cx) {
                        app.prepare_image_offer(capture, offered, reservation, origin, window, cx);
                    }
                });
            })
            .detach();
            return;
        }
        self.prepare_image_offer(capture, offered, reservation, origin, window, cx);
    }

    /// An explicit Save answer creates this missing file under a pinned workspace boundary through DocumentSession.
    /// It cannot overwrite a file created while the prompt was open or follow a replaced directory junction.
    fn save_image_source(
        &mut self,
        capture: &Capture,
        cx: &mut Context<Self>,
    ) -> Result<(), editor_core::DocumentError> {
        let index = self
            .active_text_tab_index()
            .expect("capture still targets the active document");
        let contents = self
            .text_tab(index)
            .expect("capture text capability")
            .editor
            .read(cx)
            .text()
            .to_string();
        let store =
            platform_windows::NewWorkspaceFileStore::at(self.workspace.root().to_path_buf());
        let tab = self.text_tab_mut(index).expect("capture text capability");
        debug_assert_eq!(tab.capability_revision, capture.document.revision);
        tab.session.save(&store, &contents)?;
        tab.disk_digest = Sha256::digest(contents.as_bytes()).into();
        tab.disk_state = DiskState::Synced;
        tab.overwrite_confirmed = false;
        tab.last_saved_at = Instant::now();
        let path = tab.path().to_path_buf();
        self.notify_language_document_saved(&path, contents, cx);
        cx.notify();
        Ok(())
    }

    /// Decode and read in the background; the UI thread publishes only a still-current capture.
    fn prepare_image_offer(
        &mut self,
        capture: Capture,
        offered: Vec<Offered>,
        reservation: worker::ImageOfferReservation,
        origin: HostImageOrigin,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.image_input_preparing = true;
        let prepare = cx
            .background_executor()
            .spawn(async move { prepare(offered) });
        cx.spawn_in(window, async move |app, cx| {
            let images = prepare.await;
            let _ = app.update_in(cx, |app, window, cx| {
                app.image_input_preparing = false;
                if !app.capture_current(&capture, window, cx) {
                    return;
                }
                match images {
                    Ok(images) => {
                        let worker = app.extensions.read(cx).worker.clone();
                        let _ = worker.tx.send(Work::ImageInput {
                            plugin: capture.plugin,
                            panel: capture.panel,
                            epoch: capture.epoch,
                            document: capture.document,
                            selection: capture.selection,
                            origin,
                            images,
                            reservation,
                        });
                    }
                    Err(error) => app.image_input_error(error.code, cx),
                }
            });
        })
        .detach();
    }

    /// Bounded preparation rejects another gesture explicitly, retaining the first gesture's selection.
    fn image_input_busy(&mut self, cx: &mut Context<Self>) {
        self.status = t!("editor.image_input_busy").to_string();
        cx.notify();
    }

    /// Host errors use project locale resources, independently from a guest's domain messages.
    fn image_input_error(&mut self, code: ErrorCode, cx: &mut Context<Self>) {
        let key = match code {
            ErrorCode::PermissionDenied => "editor.image_input_denied",
            ErrorCode::LimitExceeded => "preview.image_limit",
            ErrorCode::InvalidPath => "preview.image_path",
            _ => "editor.image_input_failed",
        };
        self.status = t!(key).to_string();
        cx.notify();
    }
}

/// External paths are selected by the user; format hints only filter irrelevant file drops.
fn image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|suffix| suffix.to_str())
        .is_some_and(|suffix| {
            ["png", "jpg", "jpeg", "gif", "webp", "svg"]
                .iter()
                .any(|allowed| suffix.eq_ignore_ascii_case(allowed))
        })
}

/// Preserve input ordering under byte/pixel quotas without exposing native paths to WASM.
fn prepare(offered: Vec<Offered>) -> Result<Vec<HostImageInput>, Failure> {
    let mut total = 0usize;
    let mut images = Vec::new();
    for input in offered {
        let bytes = match input {
            Offered::Bytes(bytes) => bytes,
            Offered::File(path) => {
                let text = path.to_string_lossy();
                if text.starts_with("\\\\.\\")
                    || text.starts_with("\\\\?\\")
                    || text
                        .chars()
                        .skip(2)
                        .any(|character| character == ':' || character == '\0')
                {
                    return Err(Failure::new(
                        ErrorCode::InvalidPath,
                        "Invalid offered image path",
                    ));
                }
                let file = std::fs::File::open(path)
                    .map_err(|error| Failure::new(ErrorCode::OperationFailed, error.to_string()))?;
                // Offered names do not authorize reading a device or an unbounded special stream.
                if !file.metadata().is_ok_and(|metadata| metadata.is_file()) {
                    return Err(Failure::new(
                        ErrorCode::InvalidPath,
                        "Offered image is not a regular file",
                    ));
                }
                let mut bytes = Vec::new();
                file.take(8 * 1024 * 1024 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|error| Failure::new(ErrorCode::OperationFailed, error.to_string()))?;
                Arc::new(bytes)
            }
        };
        total = total.saturating_add(bytes.len());
        if total > 32 * 1024 * 1024 {
            return Err(Failure::new(
                ErrorCode::LimitExceeded,
                "Image batch exceeds 32 MiB",
            ));
        }
        let format = crate::ui::plugin::bitmap::input_format(&bytes)?;
        images.push(HostImageInput { format, bytes });
    }
    Ok(images)
}
