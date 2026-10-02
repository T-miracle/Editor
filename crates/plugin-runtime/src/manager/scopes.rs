//! Logical workspace ownership shares package metadata while keeping live instances and files apart.
use super::*;

pub(super) struct ParkedWorkspace {
    pub(super) environment: Environment,
    trusted: bool,
    pub(super) live: BTreeMap<String, Instance>,
}

/// Canonical roots make equivalent host paths select the same persisted scope.
pub(crate) fn workspace_key(workspace: &str) -> String {
    let path = Path::new(workspace)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(workspace));
    format!("{:x}", Sha256::digest(path.to_string_lossy().as_bytes()))
}

impl Manager {
    /// Only the selected trusted workspace can publish document events to its owners.
    pub fn document_changed(&mut self, change: api::DocumentChange) {
        if self.trusted && self.workspace_open {
            for instance in self.live.values_mut() {
                instance.document_changed(change.clone());
            }
        }
    }
    /// Source overflow is propagated as a terminal error, not a deceptively complete partial stream.
    pub fn document_events_failed(&mut self, error: api::Failure) {
        for instance in self.live.values_mut() {
            instance.document_events_failed(error.clone());
        }
    }
    /// Effective publication is separate from persisted user preferences and cannot grant trust.
    pub fn published_entries(&self) -> Vec<Installed> {
        self.installed
            .values()
            .map(|entry| {
                let mut visible = entry.clone();
                visible.global_enabled = Some(entry.enabled);
                visible.enabled = self.trusted
                    && self.workspace_open
                    && (entry.enabled
                        || (entry.manifest.scope == api::InstanceScope::Workspace
                            && entry.project_enabled_in(&self.environment.workspace)));
                visible
            })
            .collect()
    }
    /// Count resources across logical workspaces and the application owner for shutdown diagnostics.
    pub fn resource_count(&self) -> usize {
        self.language_services
            .values()
            .filter_map(|item| item.service.as_ref().ok())
            .map(|service| service.process_count())
            .sum::<usize>()
            + self
                .live
                .values()
                .map(Instance::resource_count)
                .sum::<usize>()
            + self
                .parked
                .values()
                .flat_map(|scope| scope.live.values())
                .map(Instance::resource_count)
                .sum::<usize>()
    }
    /// New API private files never share the legacy global settings or opaque snapshot directory.
    pub(super) fn instance_data_directory(&self, manifest: &Manifest) -> PathBuf {
        self.data_directory_for(manifest, &self.environment)
    }

    pub(super) fn data_directory_for(
        &self,
        manifest: &Manifest,
        environment: &Environment,
    ) -> PathBuf {
        let root = self.root.join("data").join(&manifest.id);
        if manifest.protocol != 7 {
            return root;
        }
        let root = match manifest.scope {
            api::InstanceScope::Application => root.join("application"),
            api::InstanceScope::Workspace => root
                .join("workspaces")
                .join(workspace_key(&environment.workspace)),
        };
        root.join("files")
    }

    /// Switch the published workspace without discarding other logical workspace owners.
    pub fn switch_workspace(
        &mut self,
        mut environment: Environment,
        trusted: bool,
    ) -> anyhow::Result<()> {
        let path = Path::new(&environment.workspace).canonicalize()?;
        anyhow::ensure!(path.is_dir(), "Workspace must be a directory");
        environment.workspace = path.display().to_string();
        let old_key = workspace_key(&self.environment.workspace);
        let next_key = workspace_key(&environment.workspace);
        if self.workspace_open && old_key == next_key {
            return self.set_workspace_trust(trusted);
        }
        let ids = self
            .live
            .keys()
            .filter(|id| self.installed[*id].manifest.scope == api::InstanceScope::Workspace)
            .cloned()
            .collect::<Vec<_>>();
        let mut live = BTreeMap::new();
        for id in ids {
            live.insert(id.clone(), self.live.remove(&id).unwrap());
        }
        if self.workspace_open {
            // Host protocol transports are recreated when this window selects another workspace.
            self.language_services.clear();
            self.parked.insert(
                old_key,
                ParkedWorkspace {
                    environment: self.environment.clone(),
                    trusted: self.trusted,
                    live,
                },
            );
        }
        self.environment = environment;
        self.trusted = trusted;
        self.workspace_open = true;
        if let Some(mut parked) = self.parked.remove(&next_key) {
            if trusted && parked.trusted {
                self.live.append(&mut parked.live);
            } else {
                for instance in parked.live.values_mut() {
                    instance.stop();
                }
            }
        }
        self.start_scope_plugins();
        Ok(())
    }

    /// Revocation retires workspace guests immediately; application owners retain their own scope.
    pub fn set_workspace_trust(&mut self, trusted: bool) -> anyhow::Result<()> {
        // Authority is revoked even when a guest fails to save its final snapshot.
        self.trusted = trusted;
        if !trusted {
            let result = self.close_workspace(&self.environment.workspace.clone());
            self.workspace_open = true;
            result?;
        }
        if trusted {
            self.start_scope_plugins();
        }
        Ok(())
    }

    fn start_scope_plugins(&mut self) {
        if !self.trusted || !self.workspace_open {
            return;
        }
        let entries = self
            .installed
            .iter()
            .filter(|(_, entry)| {
                entry.enabled
                    || (entry.manifest.scope == api::InstanceScope::Workspace
                        && entry.project_enabled_in(&self.environment.workspace))
            })
            .map(|(id, entry)| (id.clone(), entry.enabled))
            .collect::<Vec<_>>();
        self.activate_saved_plugins(entries);
        if let Err(error) = self.save_registry() {
            for entry in self.installed.values_mut() {
                entry.error = Some(format!("{error:#}"));
            }
        }
    }

    /// Closing one scope removes only its views and resources, never the application singleton.
    pub fn close_workspace(&mut self, workspace: &str) -> anyhow::Result<()> {
        let key = workspace_key(workspace);
        let current = workspace_key(&self.environment.workspace) == key;
        if current {
            self.language_services.clear();
        }
        let (environment, mut live) = if current {
            self.workspace_open = false;
            let ids = self
                .live
                .keys()
                .filter(|id| self.installed[*id].manifest.scope == api::InstanceScope::Workspace)
                .cloned()
                .collect::<Vec<_>>();
            let mut live = BTreeMap::new();
            for id in ids {
                live.insert(id.clone(), self.live.remove(&id).unwrap());
            }
            (self.environment.clone(), live)
        } else if let Some(parked) = self.parked.remove(&key) {
            (parked.environment, parked.live)
        } else {
            return Ok(());
        };
        let mut failures = vec![];
        for (id, instance) in &mut live {
            if current && !self.trusted {
                // Revocation cannot re-enter guest code with its former authority, even to save state.
                instance.stop();
                continue;
            }
            if let Err(error) = instance
                .snapshot()
                .and_then(|snapshot| self.save_scope_snapshot(id, &environment, &snapshot))
            {
                failures.push(format!("{id}: {error:#}"));
            }
            // Cleanup is unconditional even if the guest traps while checkpointing.
            instance.stop();
        }
        anyhow::ensure!(failures.is_empty(), "{}", failures.join("; "));
        Ok(())
    }

    fn save_scope_snapshot(
        &self,
        id: &str,
        environment: &Environment,
        snapshot: &Snapshot,
    ) -> anyhow::Result<()> {
        let manifest = &self.installed[id].manifest;
        anyhow::ensure!(
            snapshot.data.len() <= manifest.storage_limit,
            "Plugin snapshot exceeds declared quota"
        );
        let data = self.data_directory_for(manifest, environment);
        let path = if manifest.protocol == 7 {
            data.parent().unwrap().join("state.json")
        } else {
            let digest = format!("{:x}", Sha256::digest(environment.workspace.as_bytes()));
            data.join(format!("state-{}.json", &digest[..16]))
        };
        atomic_write(&path, &serde_json::to_vec(snapshot)?)
    }

    /// Global disable/uninstall invalidates every outstanding workspace owner for the package.
    pub(super) fn retire_parked_plugin(&mut self, id: &str) {
        let removed = self
            .parked
            .values_mut()
            .filter_map(|scope| {
                scope
                    .live
                    .remove(id)
                    .map(|instance| (scope.environment.clone(), instance))
            })
            .collect::<Vec<_>>();
        for (environment, mut instance) in removed {
            let _ = instance
                .snapshot()
                .and_then(|snapshot| self.save_scope_snapshot(id, &environment, &snapshot));
            instance.stop();
        }
    }

    /// Parked scopes remain live and checkpointed although only the active scope is rendered.
    pub(super) fn poll_parked(&mut self) {
        for scope in self.parked.values_mut() {
            for instance in scope.live.values_mut() {
                if instance.poll().is_err() {
                    instance.stop();
                }
            }
        }
    }

    /// Shutdown retires application resources as well as all workspace resources.
    pub fn shutdown(&mut self) {
        let _ = self.checkpoint();
        for instance in self.live.values_mut() {
            instance.stop();
        }
        self.live.clear();
        let scopes = self.parked.keys().cloned().collect::<Vec<_>>();
        for key in scopes {
            if let Some(mut scope) = self.parked.remove(&key) {
                for (id, instance) in &mut scope.live {
                    let _ = instance.snapshot().and_then(|snapshot| {
                        self.save_scope_snapshot(id, &scope.environment, &snapshot)
                    });
                    instance.stop();
                }
            }
        }
        self.workspace_open = false;
        self.trusted = false;
    }
}
