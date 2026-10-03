//! Optional configuration hooks run before activation and cannot grant authority or replace explicit values.
use super::*;
use plugin_protocol::settings::{Effective, EffectiveValue, Phase, Source};

impl Instance {
    /// The hook sees only isolated files; ephemeral handles cannot escape into the activated instance.
    pub(crate) fn migrate_data(
        &mut self,
        from: u32,
        to: u32,
        snapshot: Option<Snapshot>,
    ) -> anyhow::Result<Option<Snapshot>> {
        let checkpoint = self.store.data().roots.checkpoint();
        self.store.data_mut().migrating = true;
        let result = self.call(api::Input::Event {
            panel: None,
            event: api::Notification::MigrateData {
                from,
                to,
                snapshot: snapshot.clone(),
            },
        });
        self.store.data_mut().migrating = false;
        self.store.data_mut().roots.release_since(checkpoint);
        Ok(result?.snapshot.or(snapshot))
    }
    /// After the durable directory switch, file handles resolve against the committed scope rather than its copy.
    pub(crate) fn retarget_data(&mut self, data: PathBuf) {
        self.store.data_mut().data = data;
    }
    /// Discovery is bounded by the normal fuel/deadline and cannot mutate files, processes or UI.
    pub(crate) fn prepare_language(
        &mut self,
        context: language::Context,
    ) -> anyhow::Result<language::Proposal> {
        let checkpoint = self.store.data().roots.checkpoint();
        self.store.data_mut().language_hook = true;
        self.store.data_mut().language_hook_checkpoint = Some(checkpoint);
        let reply = self.call(api::Input::Event {
            panel: None,
            event: api::Notification::LanguageService(context),
        });
        self.store.data_mut().language_hook = false;
        self.store.data_mut().language_hook_checkpoint = None;
        self.store.data_mut().roots.release_since(checkpoint);
        let reply = reply?;
        anyhow::ensure!(
            reply.views.is_empty() && reply.snapshot.is_none() && reply.configuration.is_none(),
            "LSP hook can return only a language proposal"
        );
        reply
            .language_service
            .ok_or_else(|| anyhow::anyhow!("LSP hook returned no proposal"))
    }
    /// Resolve discoveries on an isolated candidate before any active instance or saved config is changed.
    pub(crate) fn configure_settings(
        &mut self,
        manifest: &Manifest,
        mut values: Effective,
    ) -> anyhow::Result<()> {
        if manifest.settings.is_empty() {
            return Ok(());
        }
        anyhow::ensure!(
            self.store
                .data()
                .api
                .capabilities
                .contains_key("configuration"),
            "configuration was not negotiated"
        );
        anyhow::ensure!(
            !self.store.data().active,
            "Configuration requires a prepared candidate"
        );
        if manifest.settings_hook {
            let reply = self.call(api::Input::Event {
                panel: None,
                event: api::Notification::Configuration {
                    phase: Phase::Validate,
                    values: values.clone(),
                },
            })?;
            anyhow::ensure!(
                reply.views.is_empty() && reply.snapshot.is_none(),
                "Configuration validation cannot publish UI or snapshots"
            );
            let proposal = reply
                .configuration
                .ok_or_else(|| anyhow::anyhow!("Configuration hook returned no proposal"))?;
            anyhow::ensure!(
                proposal.errors.is_empty(),
                "Configuration rejected: {:?}",
                proposal.errors
            );
            anyhow::ensure!(
                proposal.discovered.len() <= 64,
                "Too many discovered settings"
            );
            for (key, value) in proposal.discovered {
                let definition = manifest
                    .settings
                    .get(&key)
                    .ok_or_else(|| anyhow::anyhow!("Undeclared discovered setting: {key}"))?;
                anyhow::ensure!(
                    definition.accepts(&value),
                    "Invalid discovered setting: {key}"
                );
                if values
                    .get(&key)
                    .is_some_and(|current| current.source == Source::Default)
                {
                    values.insert(
                        key,
                        EffectiveValue {
                            value,
                            source: Source::Discovered,
                        },
                    );
                }
            }
        }
        self.configuration = values;
        self.reapply_settings()
    }

    /// Rollback reinitializes guest memory, so the previous resolved configuration must be delivered again.
    pub(super) fn reapply_settings(&mut self) -> anyhow::Result<()> {
        if !self.configuration.is_empty() {
            self.call(api::Input::Event {
                panel: None,
                event: api::Notification::Configuration {
                    phase: Phase::Apply,
                    values: self.configuration.clone(),
                },
            })?;
        }
        Ok(())
    }
}
