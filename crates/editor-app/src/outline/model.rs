//! Immutable jobs replace the tree only after checking the current target, revision and provider lease.
use super::*;
use gpui_base::input::{FoldRange, RopeExt as _};
use plugin_runtime::plugin_protocol::{language::SourceSnapshot, structure::Node};

impl EditorApp {
    /// Resolve the independent structure role without tying it to a grammar or native language service.
    fn outline_target(&self, cx: &App) -> Option<Target> {
        if !self.session_state.workspace_trusted {
            return None;
        }
        let index = self.active_text_tab_index()?;
        let language = language::providers::language_for_path(self.tabs[index].path())?;
        let choice = language::providers::structure_provider(&language)?;
        let provider = self
            .extensions
            .read(cx)
            .structure_providers()
            .get(&choice)?
            .as_ref()
            .ok()?
            .clone();
        if !provider.is_active() {
            return None;
        }
        Some(Target {
            version: self.plugin_document_version(index).ok()?,
            provider,
        })
    }

    /// Called from the shell; unchanged snapshots do no background work and never reset dock proportions.
    pub(crate) fn sync_outline(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.outline_target(cx);
        if target != self.outline.target {
            let retain_browsing = matches!((&target, &self.outline.target), (Some(next), Some(previous))
                if next.version.id == previous.version.id && Arc::ptr_eq(&next.provider, &previous.provider));
            if !retain_browsing {
                self.outline.expansion.clear();
            } else if self.outline.snapshot.is_some() {
                self.outline.expansion.clear();
                let tree = self.outline.tree.read(cx);
                for root in (0..)
                    .map_while(|index| tree.entry(index))
                    .filter(|entry| entry.is_root())
                {
                    remember_expansion(root.item(), &mut self.outline.expansion);
                }
            }
            // A second revision can arrive while the first job has cleared its tree; keep the last displayed flags then.
            self.outline.pending.take();
            self.outline.target = target.clone();
            self.outline.snapshot = None;
            self.outline.definitions.clear();
            self.outline.current = None;
            self.outline.cursor = None;
            self.outline.error = false;
            self.outline
                .tree
                .update(cx, |tree, cx| tree.set_items(Vec::new(), cx));
            // Revoke only the provider's own editor; a newly selected Rust/plain editor keeps its existing grammar folds.
            if self
                .outline
                .fold_owner
                .as_ref()
                .is_some_and(|(editor, _)| editor != &self.editor || target.is_none())
            {
                if let Some((editor, controller)) = self.outline.fold_owner.take() {
                    controller.detach(&editor, cx);
                }
            }
            if let Some(target) = target {
                let (_, controller) = self.outline.fold_owner.get_or_insert_with(|| {
                    (
                        self.editor.clone(),
                        folding::Controller::attach(&self.editor, cx),
                    )
                });
                controller.replace(Vec::new(), &self.editor, cx);
                let source = SourceSnapshot {
                    document: target.version.clone(),
                    text: self.editor.read(cx).text().to_string(),
                };
                let provider = target.provider.clone();
                self.outline.pending = Some(cx.spawn_in(window, async move |owner, cx| {
                    let response = cx
                        .background_executor()
                        .scheduler_executor()
                        .spawn_dedicated(move |_| async move { provider.describe(source) })
                        .await;
                    let _ = owner.update_in(cx, |app, window, cx| {
                        // Dropped jobs and provider changes are checked again at the native application boundary.
                        if app.outline.target.as_ref() != Some(&target) {
                            return;
                        }
                        if app.outline_target(cx).as_ref() != Some(&target) {
                            // A trap can revoke this Arc without another document event. Clear its tree/folds in this completion.
                            app.sync_outline(window, cx);
                            cx.notify();
                            return;
                        }
                        app.outline.pending.take();
                        match response {
                            Ok(snapshot) => app.accept_outline(snapshot, window, cx),
                            Err(error) => {
                                app.outline.error = true;
                                tracing::warn!(%error, "document structure unavailable");
                                cx.notify();
                            }
                        }
                    });
                }));
            }
            self.outline_panel.update(cx, |_, cx| cx.notify());
        }
        self.follow_outline_cursor(cx);
    }

    /// Rebuild metadata and Base rows atomically; coverage, definitions and folds retain their separate purposes.
    fn accept_outline(
        &mut self,
        snapshot: StructureSnapshot,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let folds = snapshot
            .proposal
            .folds
            .iter()
            .filter_map(|range| {
                let text = self.editor.read(cx).text();
                let first = text.offset_to_position(range.start).line as usize;
                let last = text.offset_to_position(range.end - 1).line as usize;
                (last > first).then(|| FoldRange::new(first, last))
            })
            .collect();
        if let Some((editor, controller)) = &self.outline.fold_owner {
            controller.replace(folds, editor, cx);
        }
        let mut definitions = BTreeMap::new();
        let rows = rows(
            &snapshot.proposal.nodes,
            "",
            &mut definitions,
            &self.outline.expansion,
        );
        self.outline.definitions = definitions;
        self.outline.snapshot = Some(snapshot);
        self.outline
            .tree
            .update(cx, |tree, cx| tree.set_items(rows, cx));
        self.outline.cursor = None;
        self.follow_outline_cursor(cx);
        self.outline_panel.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    /// Always highlight the innermost definition; expansion/scroll is optional and independent of panel focus.
    fn follow_outline_cursor(&mut self, cx: &mut Context<Self>) {
        if self.outline.snapshot.is_none() {
            return;
        }
        let cursor = self.editor.read(cx).cursor();
        let follow = self.session_state.outline_follow_cursor;
        if self.outline.cursor == Some(cursor) && self.outline.followed == follow {
            return;
        }
        self.outline.cursor = Some(cursor);
        self.outline.followed = follow;
        self.outline.current = self
            .outline
            .definitions
            .iter()
            .filter(|(_, node)| node.range.start <= cursor && cursor < node.range.end)
            .min_by_key(|(_, node)| node.range.end - node.range.start)
            .map(|(id, _)| id.clone());
        if follow {
            let current = self.outline.current.clone();
            self.outline.tree.update(cx, |tree, cx| {
                if let Some(id) = current {
                    tree.reveal_item(&id.clone().into(), ScrollStrategy::Center, cx);
                    tree.set_selected_index(tree.index_of(&id.into()), cx);
                } else {
                    tree.set_selected_index(None, cx);
                }
            });
        }
        self.outline_panel.update(cx, |_, cx| cx.notify());
    }

    /// Validate the displayed snapshot again, then reuse native definition navigation/unfold/centering.
    pub(crate) fn navigate_outline(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.outline_target(cx) != self.outline.target {
            self.sync_outline(window, cx);
            return;
        }
        let Some(target) = &self.outline.target else {
            return;
        };
        let Some(definition) = self.outline.definitions.get(id) else {
            return;
        };
        let source = self.editor.read(cx).text().to_string();
        let range = lsp_types::Range::new(
            language::navigation::position_at_byte(&source, definition.definition.start),
            language::navigation::position_at_byte(&source, definition.definition.end),
        );
        let path = self.workspace.root().join(&target.version.path);
        if let Some(uri) = language::navigation::file_uri(&path) {
            self.open_definition_uri(&uri, Some(range), window, cx);
        }
    }

    /// Host-local visibility follows the workspace session; hiding does not destroy the dock leaf's position.
    pub(crate) fn toggle_outline(
        &mut self,
        _: &ToggleOutline,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.session_state.outline_visible = !self.session_state.outline_visible;
        self.session_state.save();
        self.dock_area.update(cx, |_, cx| cx.notify());
        self.outline_panel.update(cx, |_, cx| cx.notify());
        cx.notify();
    }
}

/// Stable tree-path IDs are UI identities; source byte ranges remain the only navigation authority.
fn rows(
    nodes: &[Node],
    parent: &str,
    definitions: &mut BTreeMap<String, Definition>,
    expansion: &BTreeMap<String, bool>,
) -> Vec<TreeItem> {
    nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            let id = format!("{parent}/{index}");
            definitions.insert(
                id.clone(),
                Definition {
                    range: node.range,
                    definition: node.definition,
                    icon: node.icon.clone(),
                },
            );
            let children = rows(&node.children, &id, definitions, expansion);
            let expanded = expansion.get(&id).copied().unwrap_or(parent.is_empty());
            TreeItem::new(id, node.name.clone())
                .children(children)
                .expanded(expanded)
        })
        .collect()
}

/// Base owns expansion flags, including keyboard edits; read its complete roots instead of shadowing its events.
fn remember_expansion(item: &TreeItem, expansion: &mut BTreeMap<String, bool>) {
    expansion.insert(item.id.to_string(), item.is_expanded());
    for child in &item.children {
        remember_expansion(child, expansion);
    }
}
