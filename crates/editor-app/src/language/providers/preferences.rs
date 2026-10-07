//! Host-local provider choices are bounded and replaced atomically, never inferred from project content.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io::Write as _, path::Path};

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct Saved {
    /// User file associations are independent of provider preferences and project trust.
    pub associations: BTreeMap<String, String>,
    pub user: BTreeMap<String, String>,
    pub projects: BTreeMap<String, BTreeMap<String, String>>,
    pub automatic: BTreeMap<String, BTreeMap<String, String>>,
}

impl Saved {
    pub fn read(root: &Path) -> anyhow::Result<Self> {
        let path = root.join("language-providers.json");
        if !path.exists() {
            return Ok(Self::default());
        }
        anyhow::ensure!(
            path.metadata()?.len() <= 1024 * 1024,
            "Provider preferences exceed quota"
        );
        let saved: Self = serde_json::from_slice(&std::fs::read(path)?)?;
        anyhow::ensure!(
            saved.associations.len() <= 512
                && saved.associations.iter().all(|(extension, language)| {
                    super::associations::valid_extension(extension)
                        && !language.is_empty()
                        && language.len() <= 128
                }),
            "Invalid file associations"
        );
        Ok(saved)
    }

    pub fn write(&self, root: &Path) -> anyhow::Result<()> {
        let bytes = serde_json::to_vec(self)?;
        anyhow::ensure!(
            bytes.len() <= 1024 * 1024,
            "Provider preferences exceed quota"
        );
        std::fs::create_dir_all(root)?;
        let mut file = tempfile::NamedTempFile::new_in(root)?;
        file.write_all(&bytes)?;
        file.as_file().sync_all()?;
        file.persist(root.join("language-providers.json"))?;
        Ok(())
    }
}
