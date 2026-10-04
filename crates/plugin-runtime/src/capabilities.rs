//! Host capability inventory; package identity never changes interface availability.
use plugin_protocol::{Manifest, api};
use semver::Version;

/// Production admission and instance preparation enforce the same current baseline.
pub(crate) fn require_current(manifest: &Manifest) -> anyhow::Result<()> {
    anyhow::ensure!(
        manifest.protocol == 7,
        "插件不兼容：协议 {} 已停止支持，请使用新版 SDK 更新插件（需要协议 7）。",
        manifest.protocol
    );
    negotiate(manifest)
        .map(|_| ())
        .map_err(|error| anyhow::anyhow!("插件 API 不兼容，请更新插件：{error:#}"))
}

/// Validate compatibility both when inspecting a package and before restoring an instance.
pub(crate) fn negotiate(manifest: &Manifest) -> anyhow::Result<api::Negotiated> {
    anyhow::ensure!(
        manifest.protocol == 7,
        "Only capability protocol 7 is supported"
    );
    let requirements = manifest
        .api
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Missing API requirements"))?;
    let available = [
        ("package.assets".into(), Version::new(1, 0, 0)),
        ("ui.native".into(), Version::new(1, 0, 0)),
        ("ui.richtext".into(), Version::new(1, 0, 0)),
        ("ui.code_highlighting".into(), Version::new(1, 0, 0)),
        ("ui.links".into(), Version::new(1, 0, 0)),
        ("ui.images".into(), Version::new(1, 0, 0)),
        ("ui.canvas".into(), Version::new(1, 1, 0)),
        ("ui.collections".into(), Version::new(1, 0, 0)),
        ("ui.grid".into(), Version::new(1, 0, 0)),
        ("plugin.services".into(), Version::new(1, 0, 0)),
        ("workspace.files".into(), Version::new(1, 1, 0)),
        ("host.sdk".into(), Version::new(1, 0, 0)),
        ("storage.private".into(), Version::new(1, 0, 0)),
        ("storage.editor".into(), Version::new(1, 0, 0)),
        ("storage.migration".into(), Version::new(1, 0, 0)),
        ("editor.documents".into(), Version::new(1, 0, 0)),
        ("editor.presentation".into(), Version::new(1, 0, 0)),
        ("editor.edit".into(), Version::new(1, 0, 0)),
        ("editor.navigation".into(), Version::new(1, 0, 0)),
        ("editor.toolbar".into(), Version::new(1, 0, 0)),
        ("editor.images".into(), Version::new(1, 0, 0)),
        ("configuration".into(), Version::new(1, 0, 0)),
        ("ui.panels".into(), Version::new(1, 0, 0)),
        ("ui.clipboard".into(), Version::new(1, 0, 0)),
        ("process".into(), Version::new(1, 3, 0)),
        ("language.lsp".into(), Version::new(1, 1, 0)),
        ("dependencies".into(), Version::new(1, 0, 0)),
    ]
    .into();
    Ok(requirements
        .negotiate(Version::new(1, 0, 0), &available)
        .map_err(anyhow::Error::msg)?)
}
