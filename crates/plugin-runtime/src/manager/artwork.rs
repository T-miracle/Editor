//! Native artwork reads only declared assets from the selected installed package version.
use super::Installed;
use std::{io::Read, path::Path};

impl Installed {
    /// Resolve a runtime-validated tool path within this exact immutable package version.
    /// Missing, escaped, oversized or unsafe SVG resources return None without granting file authority.
    pub fn tool_icon(&self, root: &Path, path: &str) -> Option<Vec<u8>> {
        let bytes = self.icon_bytes(root, path)?;
        crate::package::icons::svg(&bytes).ok()?;
        Some(bytes)
    }
    /// Resolve only a declared SVG from this installed package version.
    pub fn panel_icon(&self, root: &Path, panel_id: &str, dark: bool) -> Option<Vec<u8>> {
        let panel = self
            .manifest
            .panels
            .iter()
            .find(|panel| panel.id == panel_id)?;
        let path = if dark {
            panel.icon_dark.as_ref().or(panel.icon_light.as_ref())
        } else {
            panel.icon_light.as_ref().or(panel.icon_dark.as_ref())
        }?;
        let bytes = self.icon_bytes(root, path)?;
        std::str::from_utf8(&bytes)
            .ok()?
            .trim_start()
            .starts_with("<svg")
            .then_some(bytes)
    }

    /// Bound reads before allocating and keep canonical paths inside this immutable version owner.
    fn icon_bytes(&self, root: &Path, path: &str) -> Option<Vec<u8>> {
        if self.manifest.id.is_empty()
            || self.manifest.id.starts_with('.')
            || self.manifest.id.len() > 100
            || !self.manifest.id.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.' || byte == b'-'
            })
            || self.digest.len() != 64
            || !self.digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return None;
        }
        let version = root
            .canonicalize()
            .ok()?
            .join("packages")
            .join(&self.manifest.id)
            .join(&self.digest);
        // Redirecting the owner directory itself must not grant access to another package or location.
        if version.canonicalize().ok()? != version {
            return None;
        }
        let path = crate::instance::safe_path(&version, path, true).ok()?;
        let file = std::fs::File::open(path).ok()?;
        if file.metadata().ok()?.len() > 64 * 1024 {
            return None;
        }
        let mut bytes = Vec::new();
        file.take(64 * 1024 + 1).read_to_end(&mut bytes).ok()?;
        (bytes.len() <= 64 * 1024).then_some(bytes)
    }
}
