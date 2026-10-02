//! Host capability inventory; package identity never changes interface availability.
use plugin_protocol::{Manifest, api};
use semver::Version;

/// Validate compatibility both when inspecting a package and before restoring an instance.
pub(crate) fn negotiate(manifest: &Manifest) -> anyhow::Result<Option<api::Negotiated>> {
    if manifest.protocol != 7 {
        anyhow::ensure!(
            manifest.api.is_none() && manifest.scope == api::InstanceScope::Workspace,
            "Capability requirements require protocol 7"
        );
        return Ok(None);
    }
    let requirements = manifest
        .api
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Missing API requirements"))?;
    let available = [
        ("package.assets".into(), Version::new(1, 0, 0)),
        ("ui.native".into(), Version::new(1, 0, 0)),
        ("workspace.files".into(), Version::new(1, 0, 0)),
        ("storage.private".into(), Version::new(1, 0, 0)),
        ("editor.documents".into(), Version::new(1, 0, 0)),
        ("configuration".into(), Version::new(1, 0, 0)),
        ("ui.panels".into(), Version::new(1, 0, 0)),
    ]
    .into();
    Ok(Some(
        requirements
            .negotiate(Version::new(1, 0, 0), &available)
            .map_err(anyhow::Error::msg)?,
    ))
}
