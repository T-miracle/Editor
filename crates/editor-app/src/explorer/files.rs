//! Filesystem operations behind explorer menu commands.

use rust_i18n::t;
use std::{fs, path::Path};

fn valid_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\']) {
        return Err(t!("explorer.invalid_name").to_string());
    }
    #[cfg(target_os = "windows")]
    if name.contains(['<', '>', ':', '"', '|', '?', '*']) || name.ends_with(['.', ' ']) {
        return Err(t!("explorer.invalid_name").to_string());
    }
    #[cfg(target_os = "windows")]
    {
        let stem = name
            .split('.')
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        if matches!(
            stem.as_str(),
            "CON"
                | "PRN"
                | "AUX"
                | "NUL"
                | "COM1"
                | "COM2"
                | "COM3"
                | "COM4"
                | "COM5"
                | "COM6"
                | "COM7"
                | "COM8"
                | "COM9"
                | "LPT1"
                | "LPT2"
                | "LPT3"
                | "LPT4"
                | "LPT5"
                | "LPT6"
                | "LPT7"
                | "LPT8"
                | "LPT9"
        ) {
            return Err(t!("explorer.invalid_name").to_string());
        }
    }
    Ok(name)
}

pub(super) fn create(parent: &Path, name: &str, directory: bool) -> Result<(), String> {
    let path = parent.join(valid_name(name)?);
    if path.exists() {
        return Err(t!("explorer.exists").to_string());
    }
    if directory {
        fs::create_dir(&path)
    } else {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map(|_| ())
    }
    .map_err(|error| error.to_string())
}

pub(super) fn rename(path: &Path, name: &str) -> Result<(), String> {
    let target = path.with_file_name(valid_name(name)?);
    if target == path {
        return Ok(());
    }
    if target.exists() {
        return Err(t!("explorer.exists").to_string());
    }
    fs::rename(path, target).map_err(|error| error.to_string())
}

/// Remove one reviewed workspace entry without following a symbolic-link target.
pub(super) fn delete(root: &Path, path: &Path) -> Result<(), String> {
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    let metadata = path.symlink_metadata().map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err(t!("explorer.delete_symlink").to_string());
    }
    let resolved = path.canonicalize().map_err(|error| error.to_string())?;
    if resolved == root || !resolved.starts_with(&root) {
        return Err(t!("explorer.delete_outside_workspace").to_string());
    }
    if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
    .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_rename_preserve_existing_files() {
        let root = tempfile::tempdir().unwrap();
        create(root.path(), "empty", true).unwrap();
        create(root.path(), "note.txt", false).unwrap();
        rename(&root.path().join("note.txt"), "renamed.txt").unwrap();
        // Existing names and traversal remain rejected by the original create/rename commands.
        assert!(create(root.path(), "renamed.txt", false).is_err());
        assert!(create(root.path(), "../escape", false).is_err());
    }

    #[test]
    fn delete_removes_files_and_directories_but_never_the_root_or_external_files() {
        let root = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        let file = root.path().join("note.txt");
        let folder = root.path().join("nested");
        fs::write(&file, "note").unwrap();
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("child.txt"), "child").unwrap();
        let outside = external.path().join("outside.txt");
        fs::write(&outside, "outside").unwrap();

        assert!(delete(root.path(), root.path()).is_err());
        assert!(delete(root.path(), &outside).is_err());
        assert!(outside.exists());
        delete(root.path(), &file).unwrap();
        delete(root.path(), &folder).unwrap();
        assert!(!file.exists());
        assert!(!folder.exists());
        assert!(root.path().exists());
    }
}
