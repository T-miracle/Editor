//! New document attachments use a pinned directory and an atomic no-clobber publication.
use std::{
    fs::File,
    io::{self, Write},
    path::Path,
};

/// Create one complete sibling attachment without replacing an existing file or symlink.
/// `root` and `document` must be resolved absolute paths; `name` must be a single safe basename.
/// A failed preparation removes only its owned temporary file; a successful publication is permanent.
/// Returns `AlreadyExists` for a naming race and `PermissionDenied` for redirected/outside paths.
pub fn create_document_attachment(
    root: &Path,
    document: &Path,
    name: &str,
    bytes: &[u8],
) -> io::Result<()> {
    if Path::new(name).file_name() != Some(std::ffi::OsStr::new(name)) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Attachment name must be a basename",
        ));
    }
    let parent = document.parent().ok_or_else(denied)?;
    if root.canonicalize()? != root
        || document.canonicalize()? != document
        || !document.starts_with(root)
    {
        return Err(denied());
    }
    let _document = pin_document(document)?;
    publish_new(root, &parent.join(name), bytes)
}

/// A confirmed saved source stays a real file at its original path throughout attachment publication.
#[cfg(windows)]
fn pin_document(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(3)
        .open(path)?;
    if resolved_handle(&file)? != path || !file.metadata()?.is_file() {
        return Err(denied());
    }
    Ok(file)
}

#[cfg(not(windows))]
fn pin_document(path: &Path) -> io::Result<File> {
    let file = File::open(path)?;
    if resolved_handle(&file)? != path || !file.metadata()?.is_file() {
        return Err(denied());
    }
    Ok(file)
}

/// A user-confirmed save of a document without a disk file uses the normal DocumentSession contract.
/// It has create-only authority in one resolved workspace and refuses races with newly created user files.
pub struct NewWorkspaceFileStore {
    root: std::path::PathBuf,
}

impl NewWorkspaceFileStore {
    /// Bind create-only saving to the host-selected root; each actual write rechecks physical containment.
    pub fn at(root: std::path::PathBuf) -> Self {
        Self { root }
    }
}

impl editor_core::DocumentStore for NewWorkspaceFileStore {
    fn read_utf8(&self, path: &Path) -> Result<String, editor_core::DocumentError> {
        super::NativeFileStore.read_utf8(path)
    }

    fn write_utf8(&self, path: &Path, contents: &str) -> Result<(), editor_core::DocumentError> {
        publish_new(&self.root, path, contents.as_bytes()).map_err(|source| {
            editor_core::DocumentError::Write {
                path: path.into(),
                source,
            }
        })
    }
}

/// Pinned parent handles and a temporary file share one boundary for new documents and attachments.
fn publish_new(root: &Path, destination: &Path, bytes: &[u8]) -> io::Result<()> {
    let name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(denied)?;
    let device = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches([' ', '.'])
        .to_ascii_uppercase();
    if name.is_empty()
        || name.len() > 255
        || name.chars().any(char::is_control)
        || name.contains(['/', '\\', ':', '<', '>', '"', '|', '?', '*'])
        || name.ends_with(['.', ' '])
        || matches!(
            device.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$" | "CONIN$" | "CONOUT$"
        )
        || device
            .strip_prefix("COM")
            .or_else(|| device.strip_prefix("LPT"))
            .is_some_and(|suffix| {
                matches!(
                    suffix,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid new file name",
        ));
    }
    let parent = destination.parent().ok_or_else(denied)?;
    if root.canonicalize()? != root || parent.canonicalize()? != parent || !parent.starts_with(root)
    {
        return Err(denied());
    }
    // Windows directory handles forbid delete-sharing, preventing a rename/reparse replacement
    // between containment checks, writing the temporary file and publishing its final name.
    let pins = pin_directories(parent)?;
    if parent.canonicalize()? != parent || root.canonicalize()? != root {
        return Err(denied());
    }
    let publication_directory = publication_directory(parent, &pins);
    let mut temporary = tempfile::NamedTempFile::new_in(&publication_directory)?;
    if resolved_handle(temporary.as_file())?.parent() != Some(parent) {
        return Err(denied());
    }
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    if parent.canonicalize()? != parent || root.canonicalize()? != root {
        return Err(denied());
    }
    // persist_noclobber uses the platform's atomic create-if-absent primitive. The target is
    // never opened for truncation, including when another importer claims the same name.
    temporary
        .persist_noclobber(publication_directory.join(name))
        .map_err(|error| error.error)?;
    Ok(())
}

fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "Document attachment directory was redirected",
    )
}

/// Linux publication addresses the opened parent inode instead of following its old pathname again.
#[cfg(target_os = "linux")]
fn publication_directory(_: &Path, pins: &[File]) -> std::path::PathBuf {
    use std::os::fd::AsRawFd;
    format!("/proc/self/fd/{}", pins[0].as_raw_fd()).into()
}

#[cfg(not(target_os = "linux"))]
fn publication_directory(parent: &Path, _: &[File]) -> std::path::PathBuf {
    parent.to_path_buf()
}

/// Hold every ancestor until publication so an otherwise valid pathname cannot be retargeted.
#[cfg(windows)]
fn pin_directories(parent: &Path) -> io::Result<Vec<File>> {
    use std::os::windows::fs::OpenOptionsExt;
    parent
        .ancestors()
        .map(|path| {
            let file = std::fs::OpenOptions::new()
                .read(true)
                .share_mode(3)
                .custom_flags(0x02000000) // FILE_FLAG_BACKUP_SEMANTICS permits directory handles.
                .open(path)?;
            if resolved_handle(&file)? != path {
                return Err(denied());
            }
            Ok(file)
        })
        .collect()
}

/// Windows returns the opened object's path, rather than resolving a possibly replaced pathname.
#[cfg(windows)]
fn resolved_handle(file: &File) -> io::Result<std::path::PathBuf> {
    use std::os::windows::{ffi::OsStringExt, io::AsRawHandle};
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetFinalPathNameByHandleW(
            handle: *mut std::ffi::c_void,
            path: *mut u16,
            size: u32,
            flags: u32,
        ) -> u32;
    }
    let mut buffer = vec![0u16; 32768];
    // The file owns a live handle throughout the call, and the writable buffer has the given size.
    let count = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle(),
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            0,
        )
    };
    if count == 0 {
        return Err(io::Error::last_os_error());
    }
    if count as usize >= buffer.len() {
        return Err(denied());
    }
    Ok(std::path::PathBuf::from(std::ffi::OsString::from_wide(
        &buffer[..count as usize],
    )))
}

/// Linux keeps a directory descriptor and resolves the live temporary descriptor via procfs.
#[cfg(target_os = "linux")]
fn pin_directories(parent: &Path) -> io::Result<Vec<File>> {
    parent.ancestors().map(File::open).collect()
}

#[cfg(target_os = "linux")]
fn resolved_handle(file: &File) -> io::Result<std::path::PathBuf> {
    use std::os::fd::AsRawFd;
    std::fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd()))
}

/// This adapter refuses file publication on platforms without an opened-object path check.
#[cfg(not(any(windows, target_os = "linux")))]
fn pin_directories(_: &Path) -> io::Result<Vec<File>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Attachment directory pinning is unavailable",
    ))
}

#[cfg(not(any(windows, target_os = "linux")))]
fn resolved_handle(_: &File) -> io::Result<std::path::PathBuf> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Opened-object path resolution is unavailable",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Competing imports must publish exactly one complete file and preserve its winning bytes.
    #[test]
    fn concurrent_attachment_creation_never_overwrites() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let document = root.join("notes.md");
        std::fs::write(&document, "source").unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let jobs: Vec<_> = [b"first".as_slice(), b"second".as_slice()]
            .into_iter()
            .map(|bytes| {
                let (root, document, barrier) = (root.clone(), document.clone(), barrier.clone());
                std::thread::spawn(move || {
                    barrier.wait();
                    create_document_attachment(&root, &document, "img.png", bytes)
                })
            })
            .collect();
        let results: Vec<_> = jobs.into_iter().map(|job| job.join().unwrap()).collect();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert!(
            results
                .iter()
                .filter_map(|result| result.as_ref().err())
                .all(|error| error.kind() == io::ErrorKind::AlreadyExists)
        );
        let saved = std::fs::read(root.join("img.png")).unwrap();
        assert!(saved == b"first" || saved == b"second");
        assert_eq!(
            std::fs::read_dir(&root).unwrap().count(),
            2,
            "no incomplete temporary file remains"
        );
    }

    /// Unresolved documents and traversal cannot create files in either the workspace or its parent.
    #[test]
    fn attachment_requires_saved_document_and_safe_sibling() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let document = root.join("notes.md");
        assert!(create_document_attachment(&root, &document, "img.png", b"bytes").is_err());
        std::fs::write(&document, "source").unwrap();
        for name in ["../img.png", "img.png:stream", "nested/img.png", "img.png."] {
            assert_eq!(
                create_document_attachment(&root, &document, name, b"bytes")
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::InvalidInput
            );
        }
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
    }

    /// The create-only store preserves a file created by the first confirmed save.
    #[test]
    fn new_workspace_store_creates_once_without_overwriting() {
        use editor_core::DocumentStore as _;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let store = NewWorkspaceFileStore::at(root.clone());
        let file = root.join("notes.md");
        store.write_utf8(&file, "first").unwrap();
        assert!(store.write_utf8(&file, "second").is_err());
        assert_eq!(std::fs::read_to_string(file).unwrap(), "first");
    }
}
