//! A durable, bounded write plan resumes interrupted import without overwriting newer user data.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::Write,
    path::{Path, PathBuf},
};

/// Paths are computed by the host; a journal can select only these fixed migration outputs.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    State,
    Settings,
    Configurations,
    Layout,
    Providers,
}
#[derive(Serialize, Deserialize)]
struct WriteTarget {
    kind: Kind,
    before: Option<String>,
    after: String,
    bytes: Vec<u8>,
}
#[derive(Serialize, Deserialize)]
pub(super) struct Plan {
    version: u32,
    targets: Vec<WriteTarget>,
}
impl Default for Plan {
    fn default() -> Self {
        Self {
            version: 1,
            targets: vec![],
        }
    }
}
/// All roots come from the current application's profile, never from a journal or guest payload.
pub(super) struct Locations<'a> {
    pub native: &'a Path,
    pub configurations: &'a Path,
    pub layout: &'a Path,
    pub runtime: &'a Path,
}
impl Locations<'_> {
    fn path(&self, kind: Kind) -> PathBuf {
        match kind {
            Kind::State => self.native.join("state.json"),
            Kind::Settings => self.native.join("settings.json"),
            Kind::Configurations => self.configurations.into(),
            Kind::Layout => self.layout.into(),
            Kind::Providers => self.runtime.join("service-providers.json"),
        }
    }
}
impl Plan {
    /// Capture the prior content at preparation, so a later retry may apply only this exact edit.
    pub fn add(&mut self, locations: &Locations, kind: Kind, bytes: Vec<u8>) -> anyhow::Result<()> {
        let before = read(&locations.path(kind), 8 * 1024 * 1024)?;
        if before.as_deref() == Some(bytes.as_slice()) {
            return Ok(());
        }
        self.version = 1;
        self.targets.push(WriteTarget {
            kind,
            before: before.map(|bytes| digest(&bytes)),
            after: digest(&bytes),
            bytes,
        });
        Ok(())
    }
    /// Publish the whole plan before replacing any destination; source backups have already synced.
    pub fn stage(&self, native: &Path) -> anyhow::Result<()> {
        atomic(
            &native.join("upgrade-pending.json"),
            &serde_json::to_vec(self)?,
        )
    }
}
/// Write sync and rename happen in the same directory; failed replacement keeps the previous file.
pub(super) fn atomic(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Invalid storage destination"))?;
    std::fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path)?;
    Ok(())
}
/// Copy-once backup never replaces earlier evidence, including a damaged original later repaired.
pub(super) fn backup(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    std::fs::create_dir_all(path.parent().unwrap())?;
    let mut temporary = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    match temporary.persist_noclobber(path) {
        Ok(_) => Ok(()),
        Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error.into()),
    }
}
/// Reject nonregular and oversized inputs before allocation; missing records remain genuinely absent.
pub(super) fn read(path: &Path, limit: u64) -> anyhow::Result<Option<Vec<u8>>> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() <= limit,
        "Invalid or oversized migration file: {}",
        path.display()
    );
    let bytes = std::fs::read(path)?;
    anyhow::ensure!(
        bytes.len() as u64 <= limit,
        "Migration read exceeds capacity"
    );
    Ok(Some(bytes))
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
/// Already committed targets are skipped; changed user data is a visible conflict, never rolled back.
pub(super) fn resume(locations: &Locations) -> anyhow::Result<bool> {
    let pending = locations.native.join("upgrade-pending.json");
    let Some(bytes) = read(&pending, 64 * 1024 * 1024)? else {
        return Ok(false);
    };
    let plan: Plan = serde_json::from_slice(&bytes)?;
    anyhow::ensure!(
        plan.version == 1 && plan.targets.len() <= 5,
        "Unsupported migration journal"
    );
    // Validate the whole journal before touching any target, including duplicate destinations and
    // corrupt payloads. A durable plan must remain a single finite, reproducible transformation.
    for (index, target) in plan.targets.iter().enumerate() {
        anyhow::ensure!(
            target.bytes.len() <= 8 * 1024 * 1024
                && digest(&target.bytes) == target.after
                && !plan.targets[..index]
                    .iter()
                    .any(|other| other.kind == target.kind),
            "Invalid migration journal payload"
        );
    }
    for target in plan.targets {
        let path = locations.path(target.kind);
        let current = read(&path, 8 * 1024 * 1024)?;
        if current.as_deref() == Some(target.bytes.as_slice()) {
            continue;
        }
        anyhow::ensure!(
            current.as_ref().map(|bytes| digest(bytes)) == target.before,
            "Migration conflict; newer data retained: {}",
            path.display()
        );
        atomic(&path, &target.bytes)?;
    }
    // The completion receipt is durable only after every target. Startup with a pending plan retries.
    atomic(
        &locations.native.join("upgrade-complete.json"),
        b"{\"version\":1}",
    )?;
    std::fs::remove_file(pending)?;
    Ok(true)
}
