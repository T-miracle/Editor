//! Explicit one-workspace cutover. Startup never deletes data or migrates old configurations.
use super::{RunConfigSet, RunStoreError, SharedSet, project_path, store};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Preview only the exact local digest file and old shared run file for an authorized workspace.
/// New plugin/tree data and all other files are preserved. Malformed stores, symlinks and escaped
/// paths return an error instead of being guessed at. This function has no deletion side effects.
pub fn legacy_configuration_paths(
    base: &Path,
    workspace: &str,
    project: &Path,
) -> Result<Vec<PathBuf>, RunStoreError> {
    let mut paths = vec![];
    let local = store::file_for(base, workspace);
    if let Some(bytes) = bounded_file(&local, base)? {
        let set = RunConfigSet::from_json(&bytes)?;
        if !set.configurations.is_empty()
            && set.plugin_configurations.is_empty()
            && set.tree.folders.is_empty()
        {
            paths.push(local);
        }
    }
    let shared = project_path(project);
    if let Some(bytes) = bounded_file(&shared, project)? {
        // Shared files belong only to the removed format. Unknown/new JSON is never deleted.
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(RunStoreError::Malformed)?;
        if value.get("plugin_configurations").is_some() || value.get("tree").is_some() {
            return Ok(paths);
        }
        SharedSet::from_json(&bytes).map_err(|error| invalid(&error.to_string()))?;
        paths.push(shared);
    }
    Ok(paths)
}

/// Delete the previewed old files for exactly one authorized workspace and return removed paths.
/// Repeat calls are harmless. Recheck classification/boundaries before each removal; no directory,
/// new configuration, plugin private data, installation record or global setting is removed.
pub fn clear_legacy_configurations(
    base: &Path,
    workspace: &str,
    project: &Path,
) -> Result<Vec<PathBuf>, RunStoreError> {
    let paths = legacy_configuration_paths(base, workspace, project)?;
    let mut removed = vec![];
    for path in paths {
        if legacy_configuration_paths(base, workspace, project)?.contains(&path) {
            fs::remove_file(&path).map_err(RunStoreError::Io)?;
            removed.push(path);
        }
    }
    Ok(removed)
}

/// An exact existing regular file must stay inside its expected canonical root; traversal and
/// junctions into another workspace cannot widen this explicit deletion boundary.
fn bounded_file(path: &Path, root: &Path) -> Result<Option<Vec<u8>>, RunStoreError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(RunStoreError::Io(error)),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 1024 * 1024 {
        return Err(invalid(
            "Refusing nonregular or oversized run configuration file",
        ));
    }
    let resolved = path.canonicalize().map_err(RunStoreError::Io)?;
    let boundary = root.canonicalize().map_err(RunStoreError::Io)?;
    if !resolved.starts_with(boundary) {
        return Err(invalid(
            "Run configuration path escapes its authorized root",
        ));
    }
    fs::read(path).map(Some).map_err(RunStoreError::Io)
}
fn invalid(reason: &str) -> RunStoreError {
    RunStoreError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, reason))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ConfigurationValidation, PluginConfiguration, RunConfig, RunTarget};
    /// The isolated copy proves precise deletion, repeatability and protection of adjacent data.
    #[test]
    fn legacy_cleanup_preserves_other_workspaces_and_new_plugin_data() {
        let root = tempfile::tempdir().unwrap();
        let local = root.path().join("run");
        let project = root.path().join("authorized");
        fs::create_dir_all(&project).unwrap();
        // Adjacent private data, package registry and settings are unrelated even in a copied profile.
        let sentinels = [
            root.path().join("plugin-private.json"),
            root.path().join("registry.json"),
            root.path().join("settings.json"),
        ];
        for path in &sentinels {
            fs::write(path, b"protected bytes").unwrap();
        }
        let mut old = RunConfigSet::default();
        old.upsert(serde_json::from_value::<RunConfig>(serde_json::json!({"id":"old","name":"Old","target":{"mode":"program","program":"probe"}})).unwrap()).unwrap();
        store::save(&local, "authorized", &old).unwrap();
        store::save(&local, "main-workspace", &old).unwrap();
        let protected = fs::read(store::file_for(&local, "main-workspace")).unwrap();
        let shared = SharedSet::default();
        super::super::save_shared(&project, &shared).unwrap();
        let paths = legacy_configuration_paths(&local, "authorized", &project).unwrap();
        assert_eq!(paths.len(), 2);
        assert_eq!(
            clear_legacy_configurations(&local, "authorized", &project).unwrap(),
            paths
        );
        assert!(
            clear_legacy_configurations(&local, "authorized", &project)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            fs::read(store::file_for(&local, "main-workspace")).unwrap(),
            protected
        );
        for path in &sentinels {
            assert_eq!(fs::read(path).unwrap(), b"protected bytes");
        }
        let mut new = old;
        new.plugin_configurations.insert(
            "old".into(),
            PluginConfiguration {
                provider: "unknown-provider".into(),
                template: "command".into(),
                values: "{}".into(),
                pending_events: vec![],
                name: "New".into(),
                program: "probe".into(),
                revision: 1,
                validation: ConfigurationValidation::Unchecked,
            },
        );
        store::save(&local, "authorized", &new).unwrap();
        let bytes = fs::read(store::file_for(&local, "authorized")).unwrap();
        assert!(
            clear_legacy_configurations(&local, "authorized", &project)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            fs::read(store::file_for(&local, "authorized")).unwrap(),
            bytes
        );
        assert!(matches!(
            new.configurations[0].target,
            RunTarget::Program { .. }
        ));
    }
}
