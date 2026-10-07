//! Window draft and transaction state. Provider acknowledgements never own the user's only input copy.
use super::*;
/// Window-local canonical values and keyed native views; the persisted baseline remains in RunControls.
pub(in crate::run::ui) struct State {
    pub baseline: RunConfigSet,
    pub draft: RunConfigSet,
    pub selected: Option<String>,
    pub catalog: Vec<(String, contract::Template)>,
    pub drawer: bool,
    pub documents: BTreeMap<String, native::Document>,
    /// Native callbacks retain their form's incarnation rather than implicitly adopting a replacement.
    pub form_origins: BTreeMap<String, plugin_runtime::TargetOrigin>,
    pub views: BTreeMap<String, Entity<crate::ui::plugin::PluginView>>,
    /// Failed acknowledgements remain durable, but are not retried forever in the same window.
    pub edit_failures: BTreeSet<String>,
    pub edit_overflow: BTreeSet<String>,
    pub editing: BTreeMap<String, u64>,
    pub commit_requested: Option<CommitMode>,
    pub commit: Option<Commit>,
    pub error: Option<String>,
    pub scroll: gpui_kit::ScrollHandle,
    pub tree: Option<Entity<gpui_base::TreeState>>,
    pub tree_signature: String,
    pub tree_observer: Option<Subscription>,
    pub rename: Option<(String, Entity<gpui_base::input::InputState>)>,
    pub rename_observer: Option<Subscription>,
    pub decision: Option<Decision>,
}

impl State {
    pub fn new(set: RunConfigSet, selected: Option<String>) -> Self {
        let selected = selected.filter(|id| set.plugin_configurations.contains_key(id));
        Self {
            baseline: set.clone(),
            draft: set,
            selected,
            catalog: vec![],
            drawer: false,
            documents: BTreeMap::new(),
            form_origins: BTreeMap::new(),
            views: BTreeMap::new(),
            edit_failures: BTreeSet::new(),
            edit_overflow: BTreeSet::new(),
            editing: BTreeMap::new(),
            commit_requested: None,
            commit: None,
            error: None,
            scroll: gpui_kit::ScrollHandle::new(),
            tree: None,
            tree_signature: String::new(),
            tree_observer: None,
            rename: None,
            rename_observer: None,
            decision: None,
        }
    }
    /// Compare actual user data; rendered native controls and transient request state are not drafts.
    pub fn dirty(&self) -> bool {
        // Selection, validation receipts and executable caches are not unsaved user edits.
        fn user_data(set: &RunConfigSet) -> serde_json::Value {
            let values = set
                .plugin_configurations
                .iter()
                .map(|(id, data)| {
                    (
                        id.clone(),
                        serde_json::json!([
                            data.provider,
                            data.template,
                            data.values,
                            data.name,
                            data.pending_events
                        ]),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            serde_json::json!([set.tree.folders, set.tree.placements, values])
        }
        user_data(&self.draft) != user_data(&self.baseline) || !self.edit_overflow.is_empty()
    }
    pub fn busy(&self) -> bool {
        self.commit.is_some() || !self.editing.is_empty()
    }
}

/// Decisions stay inside the owned native window and never implicitly commit a destructive edit.
pub(in crate::run::ui) enum Decision {
    Close,
    Delete(String),
}

#[derive(Clone, Copy)]
pub(in crate::run::ui) enum CommitMode {
    Apply,
    Save,
}
pub(in crate::run::ui) struct Commit {
    pub mode: CommitMode,
    pub waiting: Vec<String>,
    pub selected: Option<String>,
}

/// Execution intent captures its original arguments; changing external selection cannot retarget a Run.
pub(in crate::run::ui) enum Execution {
    Run(Vec<plugin_runtime::RunEnvEntry>),
    Build,
    Debug,
}
impl Execution {
    pub(super) fn kind(&self) -> u8 {
        match self {
            Self::Run(_) => 0,
            Self::Build => 1,
            Self::Debug => 2,
        }
    }
}
