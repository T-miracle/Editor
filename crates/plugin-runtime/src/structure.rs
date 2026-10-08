//! Independent pure structure leases own bounded guest memory and their exact package artwork.
use crate::{Installed, Instance};
use plugin_protocol::{
    language::SourceSnapshot,
    structure::{Proposal, Provider, Request},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

/// Inspect standalone declarations through the same package gate as every other public capability.
pub(crate) fn validate_manifest(manifest: &plugin_protocol::Manifest) -> anyhow::Result<()> {
    anyhow::ensure!(
        manifest.structure_providers.len() <= 64,
        "Too many structure providers"
    );
    let mut ids = BTreeSet::new();
    for provider in &manifest.structure_providers {
        anyhow::ensure!(
            provider.valid() && ids.insert(&provider.id),
            "Invalid or duplicate structure provider"
        );
        anyhow::ensure!(
            manifest.component.is_some()
                && manifest.permissions.contains("editor.read")
                && manifest
                    .api
                    .as_ref()
                    .is_some_and(|api| api.required.contains_key("language.structure")),
            "Structure requires WASM, language.structure and editor.read"
        );
    }
    Ok(())
}

/// Safe, readonly data can cross to the native UI; unvalidated resource paths never do.
#[derive(Clone, Debug)]
pub struct StructureSnapshot {
    pub proposal: Proposal,
    /// Missing or rejected artwork is absent, so the host can render its default definition icon.
    pub icons: BTreeMap<String, Arc<[u8]>>,
}

/// A structure provider has no process or mutable document; retirement invalidates all late results.
pub struct StructureProvider {
    pub owner: String,
    pub declaration: Provider,
    pub(crate) instance: Mutex<Option<Instance>>,
    pub(crate) settings: plugin_protocol::settings::Effective,
    pub(crate) entry: Installed,
    pub(crate) root: PathBuf,
    pub(crate) active: AtomicBool,
    pub(crate) sequence: AtomicU64,
}

impl StructureProvider {
    /// One response must match the exact request/source; callers must also check their current editor version.
    /// Dropping a host job cancels delivery. Bounded guest work may finish, but cannot mutate or publish resources.
    pub fn describe(&self, source: SourceSnapshot) -> anyhow::Result<StructureSnapshot> {
        anyhow::ensure!(self.is_active(), "Structure provider has been retired");
        let request = Request {
            request: self
                .sequence
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                    value.checked_add(1)
                })
                .map_err(|_| anyhow::anyhow!("Structure request identity exhausted"))?,
            provider: self.declaration.id.clone(),
            source,
            settings: self.settings.clone(),
        };
        anyhow::ensure!(request.valid(), "Invalid structure snapshot");
        let output = {
            let mut executor = self.instance.lock().unwrap();
            // Revocation can happen while a caller waits behind another bounded invocation; never start that queued call.
            anyhow::ensure!(self.is_active(), "Structure provider has been retired");
            let result = executor
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("Structure executor has been retired"))?
                .describe_snapshot(request.clone());
            if result.is_err() {
                // Instance.call retires malformed/trapped guests. Publish that fact on the retained lease and release its memory.
                self.active.store(false, Ordering::Release);
                if let Some(mut instance) = executor.take() {
                    instance.stop();
                }
            }
            result?
        };
        anyhow::ensure!(self.is_active(), "Structure provider has been retired");
        let proposal = output
            .language_structure
            .ok_or_else(|| anyhow::anyhow!("Structure hook returned no proposal"))?;
        anyhow::ensure!(
            proposal.valid_for(&request),
            "Invalid or stale structure proposal"
        );
        // Artwork validation is host-owned. One missing icon does not suppress valid definitions or navigation.
        let mut paths = BTreeSet::new();
        let mut pending = proposal.nodes.iter().collect::<Vec<_>>();
        while let Some(node) = pending.pop() {
            pending.extend(node.children.iter());
            if let Some(icon) = &node.icon
                && icon.valid()
            {
                paths.insert(icon.light.clone());
                paths.extend(icon.dark.iter().cloned());
            }
        }
        let icons = paths
            .into_iter()
            .take(64)
            .filter_map(|path| {
                self.entry
                    .tool_icon(&self.root, &path)
                    .map(|bytes| (path, Arc::from(bytes)))
            })
            .collect();
        anyhow::ensure!(self.is_active(), "Structure provider has been retired");
        Ok(StructureSnapshot { proposal, icons })
    }

    /// A handle retained by another window cannot execute after trust, workspace or package revocation.
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    /// Mark invalid before waiting for the fuel/deadline bounded invocation; no old result can be accepted.
    pub(crate) fn retire(&self) {
        self.active.store(false, Ordering::Release);
        if let Some(mut instance) = self.instance.lock().unwrap().take() {
            instance.stop();
        }
    }
}
