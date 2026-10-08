//! Durable atomic replacement with a bounded Windows sharing-conflict recovery path.
use anyhow::Context as _;
use std::{io::Write as _, path::Path};

/// Replace in the same canonical directory; failures retain the previous file and report the destination.
/// Windows readers can temporarily deny rename, so retry only that stage without rewriting/truncating data.
pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Missing parent"))?;
    std::fs::create_dir_all(parent)?;
    let filename = path
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("Missing filename"))?;
    // Preserve extended Windows paths for both operands, including receipts/private data beyond MAX_PATH.
    let parent = parent.canonicalize()?;
    let destination = parent.join(filename);
    let mut temp = tempfile::NamedTempFile::new_in(&parent)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    #[cfg(windows)]
    let mut attempts = 0;
    loop {
        match temp.persist(&destination) {
            Ok(_) => return Ok(()),
            Err(error) => {
                #[cfg(windows)]
                {
                    use windows_sys::Win32::Foundation::{
                        ERROR_ACCESS_DENIED, ERROR_LOCK_VIOLATION, ERROR_SHARING_VIOLATION,
                    };
                    // Denied delete-sharing also returns ACCESS_DENIED. A permanent denial still fails
                    // after a 310 ms backoff budget; never change attributes/permissions or remove the destination.
                    const DELAYS_MS: [u64; 5] = [10, 20, 40, 80, 160];
                    let conflict = error.error.raw_os_error().is_some_and(|code| {
                        matches!(
                            code as u32,
                            ERROR_ACCESS_DENIED | ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION
                        )
                    });
                    if conflict && attempts < DELAYS_MS.len() {
                        temp = error.file;
                        std::thread::sleep(std::time::Duration::from_millis(DELAYS_MS[attempts]));
                        attempts += 1;
                        continue;
                    }
                }
                // Published errors must not keep an unused candidate file alive after recovery is exhausted.
                drop(error.file);
                return Err(error.error).with_context(|| {
                    format!(
                        "Cannot atomically replace plugin data file {}",
                        destination.display()
                    )
                });
            }
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::os::windows::fs::OpenOptionsExt;

    /// An indefinitely held reader must produce an actionable error without losing data or leaking a temp file.
    #[test]
    fn blocked_replacement_preserves_original_and_identifies_destination() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("snapshot.json");
        std::fs::write(&path, b"prior valid state").unwrap();
        let _reader = std::fs::OpenOptions::new()
            .read(true)
            // Allow reads/writes but deliberately exclude FILE_SHARE_DELETE.
            .share_mode(1 | 2)
            .open(&path)
            .unwrap();
        let error = atomic_write(&path, b"candidate state").unwrap_err();
        assert!(format!("{error:#}").contains("snapshot.json"));
        assert_eq!(std::fs::read(&path).unwrap(), b"prior valid state");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    /// A genuine readonly destination stays protected; recovery must never clear its attributes to force success.
    #[test]
    fn readonly_replacement_is_rejected_without_changing_attributes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("readonly.json");
        std::fs::write(&path, b"protected state").unwrap();
        let original = std::fs::metadata(&path).unwrap().permissions();
        let mut readonly = original.clone();
        readonly.set_readonly(true);
        std::fs::set_permissions(&path, readonly).unwrap();
        let result = atomic_write(&path, b"candidate state");
        let stayed_readonly = std::fs::metadata(&path).unwrap().permissions().readonly();
        // Restore only this fixture's original attributes before assertions so tempfile cleanup remains reliable.
        std::fs::set_permissions(&path, original).unwrap();
        assert!(result.is_err());
        assert!(stayed_readonly);
        assert_eq!(std::fs::read(&path).unwrap(), b"protected state");
    }
}
