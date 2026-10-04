//! Host-owned first-use choices survive plugin data deletion and never grant execution permission.
use super::*;
use std::fs::OpenOptions;

const MAX_BYTES: u64 = 1024 * 1024;
const MAX_CHOICES: usize = 4096;

/// The host's choice belongs to the opaque package identity, never a release digest or plugin data scope.
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Choice {
    Offered,
    Declined,
    Uninstalled,
}

/// A separate private file deliberately survives either uninstall data-retention policy and data rollback.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Choices {
    version: u32,
    entries: BTreeMap<String, Choice>,
}

impl Choices {
    /// Missing is the only empty-store condition; malformed, oversized or redirected metadata fails closed.
    fn read(root: &Path) -> anyhow::Result<Self> {
        let path = root.join("bundle-choices.json");
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self {
                    version: 1,
                    entries: BTreeMap::new(),
                });
            }
            Err(error) => return Err(error.into()),
        };
        anyhow::ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "Invalid bundled choices file"
        );
        anyhow::ensure!(metadata.len() <= MAX_BYTES, "Bundled choices exceed quota");
        let choices: Self = serde_json::from_slice(&std::fs::read(path)?)?;
        anyhow::ensure!(
            choices.version == 1 && choices.entries.len() <= MAX_CHOICES,
            "Invalid bundled choices version or quota"
        );
        for id in choices.entries.keys() {
            identity(id)?;
        }
        Ok(choices)
    }
}

impl Manager {
    /// Report an explicit refusal or uninstall choice; recoverable exposure alone is not a user decision.
    /// The host must treat read failures as unavailable rather than offer the package again.
    pub fn has_bundle_choice(&self, id: &str) -> anyhow::Result<bool> {
        identity(id)?;
        Ok(matches!(
            Choices::read(&self.root)?.entries.get(id),
            Some(Choice::Declined | Choice::Uninstalled)
        ))
    }

    /// First-use exposure requires the current trusted workspace, no installation and no earlier choice.
    /// A metadata failure propagates so callers cannot mistake damaged private state for a fresh profile.
    pub fn can_offer_bundle(&self, id: &str, workspace: &Path) -> anyhow::Result<bool> {
        identity(id)?;
        let choices = Choices::read(&self.root)?;
        Ok(self.bundle_workspace_active(workspace)
            && !self.installed.contains_key(id)
            && !matches!(
                choices.entries.get(id),
                Some(Choice::Declined | Choice::Uninstalled)
            ))
    }

    /// A native confirmation may consume only an offered identity in the still-selected trusted workspace.
    /// Installed, declined or uninstalled identities remain unavailable regardless of release digest.
    pub fn can_install_bundle(&self, id: &str, workspace: &Path) -> anyhow::Result<bool> {
        identity(id)?;
        let choices = Choices::read(&self.root)?;
        Ok(self.bundle_workspace_active(workspace)
            && !self.installed.contains_key(id)
            && matches!(choices.entries.get(id), Some(Choice::Offered)))
    }

    /// Canonical workspace ownership is shared with ordinary runtime scopes; a parked scope grants nothing.
    fn bundle_workspace_active(&self, workspace: &Path) -> bool {
        self.trusted
            && self.workspace_open
            && scopes::workspace_key(&workspace.to_string_lossy())
                == scopes::workspace_key(&self.environment.workspace)
    }

    /// Persist recoverable exposure before publishing native consent. Automatic withdrawal remains retryable.
    pub fn record_bundle_offer(&mut self, id: &str) -> anyhow::Result<()> {
        self.record_bundle_choice(id, Choice::Offered)
    }

    /// An explicit native refusal remains effective across restarts and future shipped release hashes.
    pub fn record_bundle_decline(&mut self, id: &str) -> anyhow::Result<()> {
        self.record_bundle_choice(id, Choice::Declined)
    }

    /// Only known installed identities can reach this path; persistence precedes registry or data removal.
    pub(super) fn record_bundle_uninstall(&mut self, id: &str) -> anyhow::Result<()> {
        anyhow::ensure!(self.installed.contains_key(id), "Unknown plugin");
        self.record_bundle_choice(id, Choice::Uninstalled)
    }

    /// Serialize independent metadata managers and atomically publish a bounded whole-file replacement.
    fn record_bundle_choice(&mut self, id: &str, choice: Choice) -> anyhow::Result<()> {
        identity(id)?;
        let path = self.root.join("bundle-choices.lock");
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) => anyhow::ensure!(
                metadata.is_file() && !metadata.file_type().is_symlink(),
                "Invalid bundled choices lock"
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)?;
        lock.try_lock()
            .map_err(|error| anyhow::anyhow!("Bundled choices are being updated: {error}"))?;
        let mut choices = Choices::read(&self.root)?;
        if matches!(choice, Choice::Offered) {
            anyhow::ensure!(
                !matches!(
                    choices.entries.get(id),
                    Some(Choice::Declined | Choice::Uninstalled)
                ),
                "Bundled identity already has a user choice"
            );
        }
        choices.entries.insert(id.into(), choice);
        anyhow::ensure!(
            choices.entries.len() <= MAX_CHOICES,
            "Bundled choices exceed quota"
        );
        let bytes = serde_json::to_vec(&choices)?;
        anyhow::ensure!(
            bytes.len() as u64 <= MAX_BYTES,
            "Bundled choices exceed quota"
        );
        atomic_write(&self.root.join("bundle-choices.json"), &bytes)
    }
}

/// Choice keys use the same opaque portable spelling admitted by current plugin packages.
fn identity(id: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !id.is_empty()
            && id.len() <= 100
            && !id.starts_with('.')
            && id.bytes().all(|byte| byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || byte == b'.'
                || byte == b'-'),
        "Invalid bundled package identity"
    );
    Ok(())
}
