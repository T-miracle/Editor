//! Reconcile declared images against the live incarnation and authoritative preview source.
use super::*;
use crate::{
    ImageResource,
    images::{Entry, Identity},
};
use std::sync::Arc;

impl Manager {
    /// Poll source-bound image tasks without IO on the manager thread and return immutable results.
    /// Keys are `plugin/panel/image/node_id`. Missing permissions and IO failures affect one image;
    /// removing a node, closing its document or retiring its owner disconnects its consumer.
    pub fn image_resources(&mut self) -> BTreeMap<String, Arc<ImageResource>> {
        self.reconcile_images();
        self.images
            .iter()
            .map(|(key, entry)| (key.clone(), entry.resource.clone()))
            .collect()
    }

    /// Views awaiting a newer Preview cannot resurrect an old image, even when its worker finishes.
    pub(super) fn reconcile_images(&mut self) {
        let mut wanted = BTreeMap::new();
        if self.trusted && self.workspace_open {
            for (plugin, instance) in &self.live {
                let Some(incarnation) = instance.instance_id() else {
                    continue;
                };
                let Some(installed) = self.installed.get(plugin) else {
                    continue;
                };
                if installed.manifest.scope != api::InstanceScope::Workspace {
                    continue;
                }
                let authorized = |permission: &str| {
                    installed.grants.contains(permission)
                        && installed.manifest.permissions.contains(permission)
                };
                if !authorized("editor.read") {
                    continue;
                }
                for (panel, view) in &instance.views {
                    let text_source = view.source.as_ref().filter(|source| {
                        instance.preview_sources.get(panel).and_then(Option::as_ref)
                            == Some(*source)
                    });
                    let file_source = view.file.as_ref().filter(|source| {
                        instance
                            .file_sources
                            .get(panel)
                            .and_then(Option::as_ref)
                            .map(|context| &context.version)
                            == Some(*source)
                    });
                    if (text_source.is_none() && file_source.is_none())
                        || !installed
                            .manifest
                            .panels
                            .iter()
                            .any(|item| item.id == *panel && item.position == "editor")
                    {
                        continue;
                    }
                    let mut visit = |node: &ui::Node| {
                        let resource = match &node.kind {
                            ui::Kind::Image { source: uri, .. } => text_source.map(|source| {
                                (api::ContentVersion::Document(source.clone()), uri.clone())
                            }),
                            ui::Kind::FileImage { .. } => file_source.map(|source| {
                                (
                                    api::ContentVersion::File(source.clone()),
                                    "@current-file".into(),
                                )
                            }),
                            _ => None,
                        };
                        if let Some((source, uri)) = resource {
                            if self.retired_image_sources.get(&format!("{plugin}/{panel}"))
                                == Some(&source)
                            {
                                return;
                            }
                            wanted.insert(
                                format!("{plugin}/{panel}/image/{}", node.id),
                                Identity {
                                    instance: incarnation.to_owned(),
                                    source,
                                    uri,
                                    workspace: self.environment.workspace.clone(),
                                    local: authorized("workspace.read"),
                                    network: authorized("network.images"),
                                },
                            );
                        }
                    };
                    view.root.visit(&mut visit);
                    if let Some(toolbar) = &view.editor_toolbar {
                        toolbar.visit(&mut visit);
                    }
                    if let Some(dialog) = &view.dialog {
                        dialog.content.visit(&mut visit);
                    }
                }
            }
        }
        // Identity includes grants and workspace. No completion crosses an update, selection or revocation.
        self.images
            .retain(|key, entry| wanted.get(key) == Some(&entry.identity));
        for (key, identity) in wanted {
            let entry = self
                .images
                .entry(key)
                .or_insert_with(|| Entry::new(identity));
            entry.poll(&self.image_budget);
        }
    }

    pub(super) fn retire_plugin_images(&mut self, plugin: &str) {
        let prefix = format!("{plugin}/");
        self.images.retain(|key, _| !key.starts_with(&prefix));
        self.retired_image_sources
            .retain(|key, _| !key.starts_with(&prefix));
    }

    /// Revoke bytes as soon as the host advances/closes a document, before a replacement UI arrives.
    pub(super) fn invalidate_document_images(&mut self, change: &api::DocumentChange) {
        for (plugin, instance) in &self.live {
            for (panel, view) in &instance.views {
                if let Some(source) = &view.source
                    && source_obsolete(&api::ContentVersion::Document(source.clone()), change)
                {
                    self.retired_image_sources
                        .insert(format!("{plugin}/{panel}"), source.clone().into());
                }
            }
        }
        self.images
            .retain(|_, entry| !source_obsolete(&entry.identity.source, change));
    }

    /// Dropping receivers retires consumers immediately; finite producer IO never blocks shutdown.
    pub(super) fn retire_workspace_images(&mut self) {
        self.images.clear();
        self.retired_image_sources.clear();
    }
}

/// Path participates in source identity even when Save As keeps the text revision unchanged.
fn source_obsolete(source: &api::ContentVersion, change: &api::DocumentChange) -> bool {
    let api::ContentVersion::Document(source) = source else {
        return false;
    };
    source.id == change.document.id
        && (change.closed
            || (change.document.revision >= source.revision && change.document != *source))
}
