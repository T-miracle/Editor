//! Virtual configuration organization. Folder identity and order never affect filesystem paths.
use super::{RunConfigError, RunConfigSet, short_digest};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Host-owned placement; ordering is meaningful only among siblings of the same category.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigurationPlacement {
    pub parent: Option<String>,
    pub order: u64,
}

/// A virtual folder has no disk path or launch policy.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigurationFolder {
    pub name: String,
}

/// Stable tree identities, bounded independently from the existing configuration quota.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigurationTree {
    pub folders: BTreeMap<String, ConfigurationFolder>,
    pub placements: BTreeMap<String, ConfigurationPlacement>,
    #[serde(default)]
    pub next_identity: u64,
}

impl RunConfigSet {
    /// Allocate once per window transaction; saved counters prevent reusing a deleted session owner.
    pub fn allocate_tree_id(&mut self, workspace: &str) -> Result<String, RunConfigError> {
        loop {
            let counter = self.tree.next_identity;
            self.tree.next_identity =
                counter
                    .checked_add(1)
                    .ok_or_else(|| RunConfigError::InvalidIdentity {
                        id: "configuration identities exhausted".into(),
                    })?;
            let id = format!("{}-node-{counter}", short_digest(workspace));
            if self.find(&id).is_none() && !self.tree.folders.contains_key(&id) {
                return Ok(id);
            }
        }
    }

    /// Existing root configurations need no migration; an absent placement means root insertion order.
    pub fn tree_parent(&self, id: &str) -> Option<String> {
        self.tree
            .placements
            .get(id)
            .and_then(|place| place.parent.clone())
    }

    /// New nodes follow a selected folder, a selected configuration's parent, or the virtual root.
    pub fn insertion_parent(&self, selected: Option<&str>) -> Option<String> {
        selected.and_then(|id| {
            if self.tree.folders.contains_key(id) {
                Some(id.into())
            } else {
                self.tree_parent(id)
            }
        })
    }

    /// Folder-first order is structural; user order remains stable within each sibling category.
    pub fn tree_children(&self, parent: Option<&str>) -> Vec<String> {
        let mut nodes = self
            .tree
            .folders
            .keys()
            .chain(self.plugin_configurations.keys())
            .filter(|id| self.tree_parent(id).as_deref() == parent)
            .cloned()
            .collect::<Vec<_>>();
        nodes.sort_by_key(|id| {
            let folder = self.tree.folders.contains_key(id);
            let order = self
                .tree
                .placements
                .get(id)
                .map(|place| place.order)
                .unwrap_or_else(|| {
                    self.configurations
                        .iter()
                        .position(|item| item.id == *id)
                        .unwrap_or(0) as u64
                });
            (!folder, order, id.clone())
        });
        nodes
    }

    /// Move or insert a node, optionally before a sibling. Invalid requests leave the tree untouched.
    pub fn place_tree_node(
        &mut self,
        id: &str,
        parent: Option<String>,
        before: Option<&str>,
    ) -> Result<(), RunConfigError> {
        let bad = || RunConfigError::InvalidIdentity {
            id: "invalid configuration tree move".into(),
        };
        if !self.tree.folders.contains_key(id) && !self.plugin_configurations.contains_key(id) {
            return Err(bad());
        }
        if let Some(parent) = &parent {
            if !self.tree.folders.contains_key(parent)
                || parent == id
                || self.tree_ancestors(parent).contains(&id.to_owned())
            {
                return Err(bad());
            }
        }
        let folder = self.tree.folders.contains_key(id);
        // Dropping before itself preserves the existing order instead of moving to the category end.
        if before == Some(id) && self.tree_parent(id) == parent {
            return Ok(());
        }
        let mut siblings = self
            .tree_children(parent.as_deref())
            .into_iter()
            .filter(|other| other != id && self.tree.folders.contains_key(other) == folder)
            .collect::<Vec<_>>();
        let index = match before {
            Some(other) if other != id => siblings
                .iter()
                .position(|sibling| sibling == other)
                .ok_or_else(bad)?,
            _ => siblings.len(),
        };
        siblings.insert(index, id.into());
        for (index, sibling) in siblings.into_iter().enumerate() {
            self.tree.placements.insert(
                sibling,
                ConfigurationPlacement {
                    parent: parent.clone(),
                    order: index as u64,
                },
            );
        }
        Ok(())
    }

    /// Return root-to-parent ancestry, bounded so malformed input cannot loop indefinitely.
    pub fn tree_ancestors(&self, id: &str) -> Vec<String> {
        let mut ancestors = vec![];
        let mut parent = self.tree_parent(id);
        while let Some(id) = parent {
            if ancestors.contains(&id) || ancestors.len() >= 128 {
                break;
            }
            parent = self.tree_parent(&id);
            ancestors.push(id);
        }
        ancestors.reverse();
        ancestors
    }

    /// Deletion previews and commits use the same closure; running sessions retain their own snapshots.
    pub fn tree_subtree(&self, id: &str) -> Vec<String> {
        let mut pending = vec![id.to_owned()];
        let mut nodes = vec![];
        while let Some(id) = pending.pop() {
            if nodes.contains(&id) {
                continue;
            }
            pending.extend(self.tree_children(Some(&id)));
            nodes.push(id);
        }
        nodes
    }

    /// Remove a confirmed virtual subtree from a draft, including metadata and external references.
    pub fn remove_tree_node(&mut self, id: &str) {
        for id in self.tree_subtree(id) {
            self.remove(&id);
            self.tree.folders.remove(&id);
            self.tree.placements.remove(&id);
        }
    }

    /// Apply only one configuration and the folder path needed to locate it.
    /// Unrelated drafts are not copied, and external selection belongs to the existing baseline.
    pub fn apply_tree_configuration(
        &mut self,
        draft: &Self,
        id: &str,
    ) -> Result<(), RunConfigError> {
        let configuration = draft
            .find(id)
            .ok_or_else(|| RunConfigError::InvalidIdentity { id: id.into() })?;
        self.upsert(configuration.clone())?;
        self.plugin_configurations
            .insert(id.into(), draft.plugin_configurations[id].clone());
        for ancestor in draft.tree_ancestors(id) {
            self.tree
                .folders
                .insert(ancestor.clone(), draft.tree.folders[&ancestor].clone());
            if let Some(place) = draft.tree.placements.get(&ancestor) {
                self.tree.placements.insert(ancestor, place.clone());
            }
        }
        match draft.tree.placements.get(id) {
            Some(place) => {
                self.tree.placements.insert(id.into(), place.clone());
            }
            None => {
                self.tree.placements.remove(id);
            }
        }
        self.tree.next_identity = self.tree.next_identity.max(draft.tree.next_identity);
        self.validate_tree()
    }

    /// Persist only bounded, acyclic trees with owned unique identities and real folder parents.
    pub(super) fn validate_tree(&self) -> Result<(), RunConfigError> {
        let bad = || RunConfigError::InvalidIdentity {
            id: "invalid configuration tree".into(),
        };
        let ids = self
            .configurations
            .iter()
            .map(|item| &item.id)
            .collect::<BTreeSet<_>>();
        if ids.len() != self.configurations.len()
            || self.tree.folders.len() > 128
            || self.tree.next_identity == u64::MAX
        {
            return Err(bad());
        }
        for (id, folder) in &self.tree.folders {
            if id.is_empty()
                || id.len() > 256
                || ids.contains(id)
                || folder.name.trim().is_empty()
                || folder.name.len() > 256
            {
                return Err(bad());
            }
        }
        for (id, place) in &self.tree.placements {
            if !self.tree.folders.contains_key(id) && !self.plugin_configurations.contains_key(id) {
                return Err(bad());
            }
            let mut cursor = place.parent.clone();
            let mut visited = BTreeSet::from([id.clone()]);
            while let Some(parent) = cursor {
                if !self.tree.folders.contains_key(&parent) || !visited.insert(parent.clone()) {
                    return Err(bad());
                }
                cursor = self.tree_parent(&parent);
            }
        }
        Ok(())
    }
}
