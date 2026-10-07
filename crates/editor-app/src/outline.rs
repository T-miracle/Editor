//! Host-owned document structure tree, native navigation and folding; plugins provide only readonly data.
mod folding;
mod model;
mod panel;
use crate::*;
pub(crate) use panel::OutlinePanel;
use plugin_runtime::{
    StructureProvider, StructureSnapshot,
    plugin_protocol::api::{DocumentVersion, TextRange},
};
use std::{collections::BTreeMap, sync::Arc};

/// Provider Arc identity and document incarnation protect against both stale revisions and reinstall cycles.
#[derive(Clone)]
struct Target {
    version: DocumentVersion,
    provider: Arc<StructureProvider>,
}
impl PartialEq for Target {
    fn eq(&self, other: &Self) -> bool {
        self.version == other.version && Arc::ptr_eq(&self.provider, &other.provider)
    }
}

/// Flat metadata augments Base tree behavior without owning any mutable document text.
#[derive(Clone)]
struct Definition {
    range: TextRange,
    definition: TextRange,
    icon: Option<plugin_runtime::plugin_protocol::structure::Icon>,
}

/// One active editor supplies the outline, even while the tree itself has keyboard focus.
pub(crate) struct OutlineState {
    pub(crate) tree: Entity<TreeState>,
    target: Option<Target>,
    snapshot: Option<StructureSnapshot>,
    definitions: BTreeMap<String, Definition>,
    /// Tree expansion is presentation state, retained across revisions of the same document/provider.
    expansion: BTreeMap<String, bool>,
    current: Option<String>,
    cursor: Option<usize>,
    followed: bool,
    error: bool,
    pending: Option<gpui_kit::Task<()>>,
    /// The controller belongs to exactly one native editor; no source or Undo state is copied.
    fold_owner: Option<(Entity<EditorState>, folding::Controller)>,
}
impl OutlineState {
    /// Keep one Base interaction state throughout repaint, panel dragging and workspace layout restoration.
    pub(crate) fn new(cx: &mut App) -> Self {
        cx.bind_keys([KeyBinding::new("enter", ActivateOutline, Some("Outline"))]);
        Self {
            tree: cx.new(|cx| TreeState::new(cx)),
            target: None,
            snapshot: None,
            definitions: BTreeMap::new(),
            expansion: BTreeMap::new(),
            current: None,
            cursor: None,
            followed: false,
            error: false,
            pending: None,
            fold_owner: None,
        }
    }
}

gpui_kit::actions!(outline, [ToggleOutline, ActivateOutline]);
