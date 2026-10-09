//! Unified resource operations keep virtual text inside ordinary native document tabs.
use super::tabs::VirtualTab;
use crate::*;
use editor_core::DocumentSession;
use plugin_runtime::{
    EditorRequest,
    plugin_protocol::api::{self, EditorOperation as Op, EditorValue as Value, ErrorCode, Failure},
};

impl EditorApp {
    /// Global edit/save commands use actual pane focus and reject provider-owned readonly sessions.
    pub(crate) fn reject_readonly_document_action(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let readonly = self.active_text_tab_index().is_some_and(|index| {
            self.tabs[index]
                .text
                .as_ref()
                .unwrap()
                .session
                .is_readonly()
        }) || self.comparison_readonly_has_focus(window, cx);
        if readonly {
            self.report_host_message(
                app::messages::MessageLevel::Warning,
                t!("status.readonly_document").to_string(),
                cx,
            );
        }
        readonly
    }

    /// A retained request must still own every virtual target when it reaches the UI thread.
    pub(crate) fn check_virtual_request(&self, request: &EditorRequest) -> Result<(), Failure> {
        let check = |document: &api::DocumentVersion| {
            if let Some(tab) = self.tabs.iter().find(|tab| {
                tab.text
                    .as_ref()
                    .is_some_and(|text| format!("{:?}", text.editor.entity_id()) == document.id)
            }) {
                if let Some(virtual_tab) = &tab.virtual_document {
                    let handle = virtual_tab.resource.handle();
                    if !virtual_tab.resource.is_live() {
                        return Err(Failure::new(
                            ErrorCode::InvalidHandle,
                            "Virtual document was revoked",
                        ));
                    }
                    if handle.instance != request.handle().instance
                        || handle.scope != request.handle().scope
                    {
                        return Err(Failure::new(
                            ErrorCode::PermissionDenied,
                            "Virtual document belongs to another instance",
                        ));
                    }
                }
            }
            Ok(())
        };
        match request.operation() {
            Op::ReadDocument { document, .. }
            | Op::RefreshVirtualDocument { document, .. }
            | Op::LocateDocument { document, .. }
            | Op::SaveDocument { document }
            | Op::ReadDocumentSelection { document }
            | Op::ReplaceDocumentRange { document, .. } => check(document),
            Op::CompareDocuments { left, right } => {
                check(left)?;
                check(right)
            }
            _ => Ok(()),
        }
    }

    /// Local opens recheck canonical physical containment; virtual opens require issued live authority.
    pub(crate) fn open_plugin_resource(
        &mut self,
        resource: &api::ResourceIdentity,
        request: &EditorRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Value, Failure> {
        let index = match resource {
            api::ResourceIdentity::Local { path } => {
                resource.validate()?;
                let target = self
                    .workspace
                    .root()
                    .join(path)
                    .canonicalize()
                    .map_err(|error| Failure::new(ErrorCode::NotFound, error.to_string()))?;
                if !target.starts_with(self.workspace.root()) {
                    return Err(Failure::new(
                        ErrorCode::PermissionDenied,
                        "Document escapes its workspace",
                    ));
                }
                if !target.is_file() {
                    return Err(Failure::new(
                        ErrorCode::UnsupportedOperation,
                        "Resource is not a text file",
                    ));
                }
                if !request.enter_side_effect() {
                    return Err(Failure::new(
                        ErrorCode::Cancelled,
                        "Document open did not execute",
                    ));
                }
                self.open_file(target.clone(), window, cx);
                self.tabs
                    .iter()
                    .position(|tab| tab.path() == target)
                    .ok_or_else(|| {
                        Failure::new(ErrorCode::OperationFailed, "Document could not be opened")
                    })?
            }
            api::ResourceIdentity::Virtual { handle } => {
                let authority = request
                    .virtual_document()
                    .filter(|authority| authority.handle() == handle && authority.is_live())
                    .ok_or_else(|| {
                        Failure::new(ErrorCode::InvalidHandle, "Virtual resource was revoked")
                    })?;
                self.tabs
                    .iter()
                    .position(|tab| {
                        tab.virtual_document.as_ref().is_some_and(|virtual_tab| {
                            virtual_tab.resource.handle() == authority.handle()
                        })
                    })
                    .ok_or_else(|| {
                        Failure::new(ErrorCode::InvalidHandle, "Virtual document is closed")
                    })?
            }
        };
        if matches!(resource, api::ResourceIdentity::Virtual { .. }) && !request.enter_side_effect()
        {
            return Err(Failure::new(
                ErrorCode::Cancelled,
                "Document open did not execute",
            ));
        }
        self.activate_tab(index, window, cx);
        self.plugin_document_info(index, cx)
            .map(Value::DocumentOpened)
    }

    /// Provider text has no disk file, watcher, history entry, language process or restoration record.
    pub(crate) fn open_plugin_virtual(
        &mut self,
        title: &str,
        language: Option<&str>,
        text: &str,
        request: &EditorRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Value, Failure> {
        let resource = request
            .virtual_document()
            .filter(|resource| resource.is_live())
            .ok_or_else(|| Failure::new(ErrorCode::InvalidHandle, "Virtual resource was revoked"))?
            .clone();
        if !request.enter_side_effect() {
            return Err(Failure::new(
                ErrorCode::Cancelled,
                "Virtual open did not execute",
            ));
        }
        let path = PathBuf::from(resource.uri());
        let native = self.create_native_text_tab(
            DocumentSession::readonly_resource(&path, text.to_owned()),
            window,
            cx,
        );
        self.tabs.push(OpenTab {
            path,
            file_id: NEXT_FILE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            file_revision: 0,
            opened_at: Instant::now(),
            file_digest: None,
            text: Some(native),
            file_error: None,
            virtual_document: Some(VirtualTab {
                resource: resource.clone(),
                title: title.into(),
                language: language.unwrap_or("text").into(),
            }),
        });
        let index = self.tabs.len() - 1;
        self.activate_tab(index, window, cx);
        let info = self.plugin_document_info(index, cx)?;
        resource.bind(info.document.clone());
        Ok(Value::DocumentOpened(info))
    }

    /// Silent readonly replacement advances the session/version explicitly because Base set_value suppresses Change.
    pub(crate) fn refresh_plugin_virtual(
        &mut self,
        request: EditorRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Op::RefreshVirtualDocument { document, text } = request.operation().clone() else {
            unreachable!()
        };
        let result = (|| {
            let index = (0..self.tabs.len())
                .find(|index| {
                    self.plugin_document_version(*index)
                        .is_ok_and(|version| version == document)
                })
                .ok_or_else(|| {
                    Failure::new(
                        ErrorCode::StaleRevision,
                        "Virtual document changed or closed",
                    )
                })?;
            if self.tabs[index].virtual_document.is_none() {
                return Err(Failure::new(
                    ErrorCode::UnsupportedOperation,
                    "Only virtual content may be refreshed",
                ));
            }
            if !request.enter_side_effect() {
                return Err(Failure::new(
                    ErrorCode::Cancelled,
                    "Refresh did not execute",
                ));
            }
            let tab = self.tabs[index].text.as_mut().unwrap();
            // Base clears the readonly display's IME/selection/undo state, without emitting a user edit.
            tab.editor
                .update(cx, |editor, cx| editor.set_value(text, window, cx));
            tab.capability_revision = tab.capability_revision.saturating_add(1);
            tab.session.accept_disk_reload();
            let info = self.plugin_document_info(index, cx)?;
            self.tabs[index]
                .virtual_document
                .as_ref()
                .unwrap()
                .resource
                .bind(info.document.clone());
            self.sync_plugin_documents(cx);
            Ok(Value::DocumentOpened(info))
        })();
        request.finish(result);
        cx.notify();
    }

    /// Runtime retirement revokes retained authority immediately; the next native frame removes its tab.
    pub(crate) fn sync_virtual_documents(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let revoked = self
            .tabs
            .iter()
            .enumerate()
            .filter(|(_, tab)| {
                tab.virtual_document
                    .as_ref()
                    .is_some_and(|virtual_tab| !virtual_tab.resource.is_live())
            })
            .map(|(index, tab)| (index, tab.path().to_path_buf()))
            .collect::<Vec<_>>();
        if !revoked.is_empty() {
            self.editor_panel.update(cx, |_, cx| cx.notify());
        }
        for (index, path) in revoked.into_iter().rev() {
            self.remove_tab(index, path, window, cx);
        }
    }

    /// Strict UTF-16 conversion rejects split surrogate positions before changing native selection.
    pub(crate) fn locate_plugin_resource(
        &mut self,
        document: &api::DocumentVersion,
        position: api::TextPosition,
        request: &EditorRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Value, Failure> {
        let index = (0..self.tabs.len())
            .find(|index| {
                self.plugin_document_version(*index)
                    .is_ok_and(|version| version == *document)
            })
            .ok_or_else(|| Failure::new(ErrorCode::StaleRevision, "Document changed or closed"))?;
        let editor = self.tabs[index].text.as_ref().unwrap().editor.clone();
        let offset = position.offset_chars(editor.read(cx).text().chars())?;
        if !request.enter_side_effect() {
            return Err(Failure::new(
                ErrorCode::Cancelled,
                "Location did not execute",
            ));
        }
        self.activate_tab(index, window, cx);
        editor.update(cx, |editor, cx| {
            editor.set_selected_range(offset..offset, cx)
        });
        Ok(Value::DocumentLocated {
            document: document.clone(),
        })
    }
}
