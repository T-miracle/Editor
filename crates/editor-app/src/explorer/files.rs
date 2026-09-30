//! Filesystem operations behind explorer menu commands.

use gpui_kit::{ClipboardEntry, ClipboardItem};
use rust_i18n::t;
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

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

pub(super) fn clipboard_paths(item: &ClipboardItem) -> Vec<PathBuf> {
    item.entries()
        .iter()
        .filter_map(|entry| match entry {
            ClipboardEntry::ExternalPaths(paths) => Some(paths.paths().iter().cloned()),
            _ => None,
        })
        .flatten()
        .collect()
}

pub(super) fn paste(sources: &[PathBuf], destination: &Path) -> Result<(), String> {
    if sources.is_empty() {
        return Err(t!("explorer.no_clipboard_files").to_string());
    }
    // Check every top-level target before copying so a later collision does not leave earlier copies behind.
    let mut targets = HashSet::new();
    for source in sources {
        let name = source
            .file_name()
            .ok_or_else(|| t!("explorer.invalid_source").to_string())?;
        let target = destination.join(name);
        if target.exists() || !targets.insert(target.clone()) {
            return Err(t!("explorer.target_exists", path = target.display()).to_string());
        }
        if !source.exists() {
            return Err(t!("explorer.source_missing", path = source.display()).to_string());
        }
        if source
            .symlink_metadata()
            .map_err(|error| error.to_string())?
            .file_type()
            .is_symlink()
        {
            return Err(t!("explorer.symlink", path = source.display()).to_string());
        }
        if source.is_dir() && destination.starts_with(source) {
            return Err(t!("explorer.self_paste").to_string());
        }
    }
    for source in sources {
        let name = source
            .file_name()
            .ok_or_else(|| t!("explorer.invalid_source").to_string())?;
        let target = destination.join(name);
        if source.is_file() {
            fs::copy(source, &target).map_err(|error| error.to_string())?;
        } else if source.is_dir() {
            copy_directory(source, &target)?;
        } else {
            return Err(t!("explorer.source_missing", path = source.display()).to_string());
        }
    }
    Ok(())
}

fn copy_directory(source: &Path, target: &Path) -> Result<(), String> {
    fs::create_dir(target).map_err(|error| error.to_string())?;
    for entry in fs::read_dir(source).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let child = target.join(entry.file_name());
        if path
            .symlink_metadata()
            .map_err(|error| error.to_string())?
            .file_type()
            .is_symlink()
        {
            return Err(t!("explorer.symlink", path = path.display()).to_string());
        }
        if path.is_dir() {
            copy_directory(&path, &child)?;
        } else {
            fs::copy(&path, &child).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
pub(super) fn copy_to_clipboard(path: &Path) -> Result<(), String> {
    use std::{mem, os::windows::ffi::OsStrExt, ptr};
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetForegroundWindow() -> *mut ();
        fn OpenClipboard(hwnd: *mut ()) -> i32;
        fn EmptyClipboard() -> i32;
        fn SetClipboardData(format: u32, handle: *mut ()) -> *mut ();
        fn CloseClipboard() -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalAlloc(flags: u32, size: usize) -> *mut ();
        fn GlobalLock(handle: *mut ()) -> *mut u8;
        fn GlobalUnlock(handle: *mut ()) -> i32;
        fn GlobalFree(handle: *mut ()) -> *mut ();
    }
    if !path.exists() {
        return Err(t!("explorer.source_missing", path = path.display()).to_string());
    }
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0, 0]).collect();
    // DROPFILES is 20 bytes. pFiles=20 and fWide=1; the path list is double-null terminated.
    let header_size = 20usize;
    let size = header_size + wide.len() * mem::size_of::<u16>();
    unsafe {
        let memory = GlobalAlloc(0x0042, size); // GMEM_MOVEABLE | GMEM_ZEROINIT
        if memory.is_null() {
            return Err(t!("explorer.clipboard_failed").to_string());
        }
        let bytes = GlobalLock(memory);
        if bytes.is_null() {
            GlobalFree(memory);
            return Err(t!("explorer.clipboard_failed").to_string());
        }
        ptr::write_unaligned(bytes.cast::<u32>(), header_size as u32);
        ptr::write_unaligned(bytes.add(16).cast::<u32>(), 1);
        ptr::copy_nonoverlapping(
            wide.as_ptr().cast::<u8>(),
            bytes.add(header_size),
            wide.len() * 2,
        );
        GlobalUnlock(memory);
        // EmptyClipboard requires a real owner window before SetClipboardData can transfer memory.
        let owner = GetForegroundWindow();
        if owner.is_null() || OpenClipboard(owner) == 0 {
            GlobalFree(memory);
            return Err(t!("explorer.clipboard_failed").to_string());
        }
        let success = EmptyClipboard() != 0 && !SetClipboardData(15, memory).is_null(); // CF_HDROP
        CloseClipboard();
        if !success {
            GlobalFree(memory);
            return Err(t!("explorer.clipboard_failed").to_string());
        }
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub(super) fn copy_to_clipboard(_: &Path) -> Result<(), String> {
    Err(t!("explorer.clipboard_unsupported").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_rename_and_paste_preserve_existing_files() {
        let root = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        create(root.path(), "empty", true).unwrap();
        create(root.path(), "note.txt", false).unwrap();
        rename(&root.path().join("note.txt"), "renamed.txt").unwrap();
        fs::write(external.path().join("source.txt"), "source").unwrap();
        paste(
            &[external.path().join("source.txt")],
            &root.path().join("empty"),
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(root.path().join("empty/source.txt")).unwrap(),
            "source"
        );
        assert!(
            paste(
                &[external.path().join("source.txt")],
                &root.path().join("empty")
            )
            .is_err()
        );
        let other = tempfile::tempdir().unwrap();
        fs::write(other.path().join("source.txt"), "other").unwrap();
        create(root.path(), "batch", true).unwrap();
        assert!(
            paste(
                &[
                    external.path().join("source.txt"),
                    other.path().join("source.txt"),
                ],
                &root.path().join("batch")
            )
            .is_err()
        );
        assert!(!root.path().join("batch/source.txt").exists());
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
