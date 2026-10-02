//! Optional configuration hooks run before activation and cannot grant authority or replace explicit values.
use super::*;
use plugin_protocol::settings::{Effective, EffectiveValue, Phase, Source};

impl Instance {
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
                .as_ref()
                .is_some_and(|api| api.capabilities.contains_key("configuration")),
            "configuration was not negotiated"
        );
        anyhow::ensure!(
            !self.store.data().active,
            "Configuration requires a prepared candidate"
        );
        if manifest.settings_hook {
            let reply = self.call(Message::Event(Event::Capability(
                api::Notification::Configuration {
                    phase: Phase::Validate,
                    values: values.clone(),
                },
            )))?;
            anyhow::ensure!(
                reply.scene.is_none() && reply.scenes.is_empty() && reply.snapshot.is_none(),
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
            self.call(Message::Event(Event::Capability(
                api::Notification::Configuration {
                    phase: Phase::Apply,
                    values: self.configuration.clone(),
                },
            )))?;
        }
        Ok(())
    }
}
