//! File identities and optional native text sessions shared by document lifecycle operations.
use editor_core::DocumentSession;
use gpui_base::input::{EditorState, TextDecorationCollection};
use gpui_kit::{Entity, Subscription};
use std::{
    path::{Path, PathBuf},
    time::Instant,
};

/// Reopening a path issues a fresh identity, preventing delayed results from targeting its new tab.
pub(crate) static NEXT_FILE_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// A file tab owns its identity independently of native text editing resources.
pub(crate) struct OpenTab {
    pub(crate) path: PathBuf,
    pub(crate) file_id: u64,
    pub(crate) file_revision: u64,
    /// Background checks started before this incarnation opened cannot reload it.
    pub(crate) opened_at: Instant,
    /// The watcher retains a hash rather than a second mutable copy of image bytes.
    pub(crate) file_digest: Option<[u8; 32]>,
    pub(crate) text: Option<TextTab>,
    /// File failures retain the tab and its retry target, including unavailable viewer authority.
    pub(crate) file_error: Option<String>,
}

impl OpenTab {
    /// File navigation does not require consulting a text editing session.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
    /// Read-only files never acquire a dirty text revision or a text save obligation.
    pub(crate) fn is_dirty(&self) -> bool {
        self.text
            .as_ref()
            .is_some_and(|text| text.session.is_dirty())
    }
    /// Native entity matching excludes binary tabs rather than lending the previously active editor.
    pub(crate) fn owns_editor(&self, editor: &Entity<EditorState>) -> bool {
        self.text
            .as_ref()
            .is_some_and(|text| text.editor == *editor)
    }
    /// Match callbacks to their original text entity, excluding file-only tabs.
    pub(crate) fn owns_editor_id(&self, id: gpui_kit::EntityId) -> bool {
        self.text
            .as_ref()
            .is_some_and(|text| text.editor.entity_id() == id)
    }
}

/// Native text state retains its session, selection/IME entity and derived diagnostics.
pub(crate) struct TextTab {
    /// Unlike the dirty revision, this also advances on disk reloads and other programmatic changes.
    pub(crate) capability_revision: u64,
    pub(crate) session: DocumentSession,
    pub(crate) editor: Entity<EditorState>,
    /// Hash of the last disk text, avoiding a second full copy of every open document.
    pub(crate) disk_digest: [u8; 32],
    /// Ignore worker reads that began before the latest successful local save.
    pub(crate) last_saved_at: Instant,
    pub(crate) disk_state: DiskState,
    pub(crate) suppress_change: bool,
    pub(crate) overwrite_confirmed: bool,
    /// A separate decoration layer keeps a definition jump visible for two seconds.
    pub(crate) definition_highlight: TextDecorationCollection,
    pub(crate) definition_highlight_generation: u64,
    /// Diagnostics retain only derived parser state; EditorState owns the editable text.
    pub(crate) diagnostics: super::diagnostics::DocumentDiagnostics,
    pub(crate) _subscription: Subscription,
    pub(crate) _observer: Subscription,
}

impl TextTab {
    /// Text-specific operations use the session's canonical path after narrowing the file capability.
    pub(crate) fn path(&self) -> &Path {
        self.session.path()
    }
}

/// Open tabs remain present when their backing file changes or disappears.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DiskState {
    Synced,
    Conflict,
    Deleted,
}
