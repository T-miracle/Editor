//! Read current metadata and text only from the owning native DocumentSession/EditorState pair.
use super::*;
use crate::editor::language_for_path;

impl EditorApp {
    /// Publish metadata transitions without retaining text or replacing the legacy revision stream.
    pub(super) fn sync_plugin_document_metadata(&mut self, cx: &mut Context<Self>) {
        let current = (0..self.tabs.len())
            .filter_map(|index| self.plugin_document_info(index, cx).ok())
            .map(|info| (info.document.id.clone(), info))
            .collect::<BTreeMap<_, _>>();
        let previous = std::mem::replace(&mut self.plugin_document_metadata, current.clone());
        for (id, info) in &current {
            match previous.get(id) {
                None => self.publish_document_event(
                    Some(info.resource.clone()),
                    api::DocumentEventKind::Opened(info.clone()),
                    cx,
                ),
                Some(old) => {
                    if old.document.revision != info.document.revision {
                        self.publish_document_event(
                            Some(info.resource.clone()),
                            api::DocumentEventKind::ContentChanged(info.clone()),
                            cx,
                        );
                    }
                    if old.selection != info.selection {
                        self.publish_document_event(
                            Some(info.resource.clone()),
                            api::DocumentEventKind::SelectionChanged {
                                document: info.document.clone(),
                                selection: info.selection,
                            },
                            cx,
                        );
                    }
                    if old.visible_rows != info.visible_rows {
                        self.publish_document_event(
                            Some(info.resource.clone()),
                            api::DocumentEventKind::ViewportChanged {
                                document: info.document.clone(),
                                visible_rows: info.visible_rows,
                            },
                            cx,
                        );
                    }
                }
            }
        }
        for (id, info) in &previous {
            if !current.contains_key(id) {
                self.publish_document_event(
                    Some(info.resource.clone()),
                    api::DocumentEventKind::Closed(info.document.clone()),
                    cx,
                );
            }
        }
        let active = self
            .active_tab_index()
            .and_then(|index| self.plugin_document_info(index, cx).ok());
        let version = active.as_ref().map(|info| info.document.clone());
        let changed = match (&self.plugin_active_document, &version) {
            (Some(old), Some(current)) => old.id != current.id || old.path != current.path,
            (None, None) => false,
            _ => true,
        };
        self.plugin_active_document = version.clone();
        if changed {
            self.publish_document_event(
                active.map(|info| info.resource),
                api::DocumentEventKind::ActiveChanged(version),
                cx,
            );
        }
    }

    /// Source sequence order survives background publication; save notifications are observations.
    pub(crate) fn publish_document_event(
        &mut self,
        resource: Option<api::ResourceIdentity>,
        kind: api::DocumentEventKind,
        cx: &App,
    ) {
        if !self.session_state.workspace_trusted {
            return;
        }
        self.plugin_document_sequence = self.plugin_document_sequence.saturating_add(1);
        self.extensions
            .read(cx)
            .worker
            .state
            .lock()
            .unwrap()
            .document_stream
            .push(api::DocumentEvent {
                sequence: self.plugin_document_sequence,
                resource,
                kind,
            });
    }

    /// Save hooks capture the exact session identity and never borrow authority from a disk path.
    pub(crate) fn publish_document_save(
        &mut self,
        index: usize,
        result: Option<Result<(), api::Failure>>,
        cx: &App,
    ) {
        if let Ok(info) = self.plugin_document_info(index, cx) {
            let kind = match result {
                None => api::DocumentEventKind::WillSave(info.document),
                Some(result) => api::DocumentEventKind::DidSave {
                    document: info.document,
                    result,
                },
            };
            self.publish_document_event(Some(info.resource), kind, cx);
        }
    }

    /// Identity, path and revision are checked together; close/reopen can never satisfy an old target.
    pub(super) fn plugin_document_target(
        &self,
        document: &DocumentVersion,
    ) -> Result<usize, Failure> {
        (0..self.tabs.len())
            .find(|index| {
                self.plugin_document_version(*index)
                    .is_ok_and(|current| current == *document)
            })
            .ok_or_else(|| {
                Failure::new(
                    ErrorCode::StaleRevision,
                    "Document was changed, renamed or closed",
                )
            })
    }

    /// Range reads share the metadata revision and use strict SDK coordinates; disk text is irrelevant.
    pub(super) fn read_plugin_document(
        &self,
        document: &DocumentVersion,
        range: Option<api::DocumentRange>,
        cx: &App,
    ) -> Result<Value, Failure> {
        let index = self.plugin_document_target(document)?;
        let info = self.plugin_document_info(index, cx)?;
        let editor = self.tabs[index].text.as_ref().unwrap().editor.read(cx);
        let text = editor.text();
        let (start, end) = match range.unwrap_or(api::DocumentRange::Bytes {
            start: 0,
            end: text.len(),
        }) {
            api::DocumentRange::Bytes { start, end } => (start, end),
            api::DocumentRange::Utf16 { start, end } => (
                start.offset_chars(text.chars())?,
                end.offset_chars(text.chars())?,
            ),
        };
        if start > end
            || end > text.len()
            || text.char_to_byte_idx(text.byte_to_char_idx(start)) != start
            || text.char_to_byte_idx(text.byte_to_char_idx(end)) != end
        {
            return Err(Failure::new(
                ErrorCode::InvalidRequest,
                "Range is outside text or splits a UTF-8 character",
            ));
        }
        let range = api::TextRange { start, end };
        if range.end - range.start > 256 * 1024 {
            return Err(Failure::new(
                ErrorCode::LimitExceeded,
                "Snapshot exceeds 256 KiB; request a smaller range",
            ));
        }
        // Allocate only the bounded requested slice; a large background file is never copied in full.
        let snapshot = text.slice(range.start..range.end).to_string();
        Ok(Value::DocumentSnapshot(api::DocumentSnapshot {
            info,
            range,
            text: snapshot,
        }))
    }

    /// Read one immutable description at the current capability revision without consulting disk text.
    pub(crate) fn plugin_document_info(
        &self,
        index: usize,
        cx: &App,
    ) -> Result<api::DocumentInfo, Failure> {
        let document = self.plugin_document_version(index)?;
        let file = &self.tabs[index];
        let tab = file.text.as_ref().ok_or_else(|| {
            Failure::new(
                ErrorCode::UnsupportedOperation,
                "Resource has no text session",
            )
        })?;
        let editor = tab.editor.read(cx);
        // Immutable revision-keyed metadata avoids rescanning every unchanged file on a selection
        // or viewport observation. No text value or undo state is cached here.
        let (encoding, eol, byte_len) = if let Some(info) = self
            .plugin_document_metadata
            .get(&document.id)
            .filter(|info| info.document == document)
        {
            (info.encoding, info.eol, info.byte_len)
        } else {
            let text = editor.text();
            (
                if text.get_char(0) == Ok('\u{feff}') {
                    api::DocumentEncoding::Utf8Bom
                } else {
                    api::DocumentEncoding::Utf8
                },
                api::DocumentEol::of_chars(text.chars()),
                text.len(),
            )
        };
        let selection = editor.selected_range();
        Ok(api::DocumentInfo {
            resource: file
                .virtual_document
                .as_ref()
                .map(|virtual_tab| api::ResourceIdentity::Virtual {
                    handle: virtual_tab.resource.handle().clone(),
                })
                .unwrap_or_else(|| api::ResourceIdentity::Local {
                    path: document.path.clone(),
                }),
            document,
            title: file.title(),
            access: api::DocumentAccess {
                read: true,
                edit: editor.presentation().is_editable(),
                save: !tab.session.is_readonly(),
            },
            dirty: tab.session.is_dirty(),
            language: file
                .virtual_document
                .as_ref()
                .map(|virtual_tab| virtual_tab.language.clone())
                .unwrap_or_else(|| language_for_path(file.path())),
            encoding,
            eol,
            byte_len,
            selection: api::TextRange {
                start: selection.start,
                end: selection.end,
            },
            visible_rows: editor.visible_row_range().map(|range| api::VisibleRows {
                start: range.start,
                end: range.end,
            }),
        })
    }
}
