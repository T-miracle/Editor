//! Owns click selection and folder expansion without filesystem rescans.

use super::tree::{collapse_descendants, root_items};
use crate::app::messages::MessageLevel;
use crate::*;

impl EditorApp {
    /// Recursively change every directory, including hidden and empty folders, without disk scans.
    pub(crate) fn set_all_explorer_directories_expanded(
        &mut self,
        expanded: bool,
        cx: &mut Context<Self>,
    ) {
        let roots = root_items(self.tree_state.read(cx));
        let directories = self
            .workspace_snapshot
            .as_ref()
            .map(|snapshot| {
                snapshot
                    .directories
                    .iter()
                    .map(PathBuf::as_path)
                    .collect::<std::collections::HashSet<_>>()
            })
            .unwrap_or_default();
        fn visit(
            items: &[TreeItem],
            root: &Path,
            directories: &std::collections::HashSet<&Path>,
            expanded: bool,
        ) {
            for item in items {
                let path = Path::new(item.id.as_str());
                if path == root || directories.contains(path) || !item.children.is_empty() {
                    item.clone().expanded(expanded);
                }
                visit(&item.children, root, directories, expanded);
            }
        }
        visit(&roots, self.workspace.root(), &directories, expanded);
        self.tree_state.update(cx, |state, cx| {
            // Reflatten once; preserve visible selection or fall back to the project root.
            let selected = state.selected_item().cloned();
            state.set_items(roots.clone(), cx);
            let index = selected
                .as_ref()
                .and_then(|item| state.index_of(&item.id))
                .or_else(|| roots.first().and_then(|item| state.index_of(&item.id)));
            // Selecting by index avoids revealing hidden ancestors after collapse-all.
            state.set_selected_index(index, cx);
        });
        self.capture_explorer_state(cx);
        self.persist_session();
        cx.notify();
    }

    /// Manually reveal the active document regardless of the automatic tab-switch preference.
    pub(crate) fn reveal_active_file_in_explorer(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let error = match self.active_path.as_ref() {
            None => Some(t!("explorer.reveal_no_active_file").to_string()),
            Some(path) if !path.starts_with(self.workspace.root()) => {
                Some(t!("explorer.reveal_outside_workspace").to_string())
            }
            Some(path)
                if !self.workspace_snapshot.as_ref().is_some_and(|snapshot| {
                    snapshot
                        .files
                        .iter()
                        .any(|file| &file.absolute_path == path)
                }) =>
            {
                Some(t!("explorer.reveal_unavailable_file").to_string())
            }
            Some(_) => None,
        };
        if let Some(message) = error {
            // Informational failures use the local nonmodal card without changing focus.
            let title = t!("explorer.reveal_failed").to_string();
            // The replaceable card may expire, but its user-facing result remains in host history.
            self.record_host_message(MessageLevel::Info, format!("{title}: {message}"), cx);
            self.notification =
                Some(cx.new(|cx| ui::controls::Notification::new(title, message, cx)));
            cx.notify();
            return;
        }
        if let Some(path) = self.active_path.clone() {
            // Reuse the live tree to expand ancestors, select the file and scroll it into view.
            self.select_file_in_tree(&path, cx);
            self.tree_state
                .update(cx, |state, cx| state.focus(window, cx));
            self.capture_explorer_state(cx);
            self.persist_session();
            cx.notify();
        }
    }

    /// Save the live hierarchy at exit even if its latest expansion event has not been delivered.
    pub(crate) fn capture_explorer_state(&mut self, cx: &App) {
        fn collect_expanded(items: &[TreeItem], expanded: &mut Vec<String>) {
            for item in items {
                if item.is_expanded() {
                    expanded.push(item.id.to_string());
                    // A pending keyboard collapse may still leave hidden children marked expanded.
                    collect_expanded(&item.children, expanded);
                }
            }
        }
        let roots = root_items(self.tree_state.read(cx));
        // Preserve the existing defaults if shutdown happens before the tree is constructed.
        if let Some(root) = roots.first() {
            self.session_state.explorer_root_expanded = root.is_expanded();
            let mut expanded = Vec::new();
            collect_expanded(&roots, &mut expanded);
            self.session_state.expanded_directories = expanded;
        }
    }

    /// Single presses select rows while suppressing the base tree's automatic folder toggle.
    pub(crate) fn select_explorer_row(
        &self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.tree_state.update(cx, |state, cx| {
            state.set_selected_index(Some(index), cx);
            state.focus(window, cx);
        });
    }

    /// Toggle by stable path so a preceding selection or background update cannot target another row.
    pub(crate) fn toggle_explorer_directory(&mut self, path: &Path, cx: &mut Context<Self>) {
        let roots = root_items(self.tree_state.read(cx));
        let Some(item) = find_tree_item(&roots, path).cloned() else {
            return;
        };
        let expanded = !item.is_expanded();
        item.clone().expanded(expanded);
        if !expanded {
            collapse_descendants(&item);
        }
        self.tree_state.update(cx, |state, cx| {
            // Reflatten the existing tree; selecting this folder never reveals its descendants.
            state.set_items(roots, cx);
            state.set_selected_item(Some(&item), cx);
            cx.emit(if expanded {
                TreeEvent::Expanded(item.id.clone())
            } else {
                TreeEvent::Collapsed(item.id.clone())
            });
        });
    }
}

#[cfg(test)]
mod tests;
