//! Locate installed resources and protect a running GUI from installer replacement.

use std::path::{Path, PathBuf};

/// Return shipped plugin directories before the source-tree fallback used by direct Cargo launches.
/// macOS bundles store resources outside `MacOS`; Windows and Linux keep them beside the binary.
pub(crate) fn shipped_plugin_roots() -> Vec<PathBuf> {
    preferred_root(resource_roots(
        std::env::current_exe().ok().as_deref(),
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins"),
    ))
}

/// An installed resource directory is authoritative, even if damaged; never mix old developer ZIPs.
/// The last candidate is always the source fallback, used only when no installed location exists.
fn preferred_root(candidates: Vec<PathBuf>) -> Vec<PathBuf> {
    let installed = candidates
        .iter()
        .take(candidates.len().saturating_sub(1))
        .find(|path| path.is_dir());
    installed
        .or_else(|| candidates.last())
        .cloned()
        .into_iter()
        .collect()
}

/// Infer only our explicit bundle layout; project settings cannot redirect installation resources.
fn resource_roots(executable: Option<&Path>, development: PathBuf) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(parent) = executable.and_then(Path::parent) {
        if parent.file_name().is_some_and(|name| name == "MacOS")
            && let Some(contents) = parent.parent()
            && contents.file_name().is_some_and(|name| name == "Contents")
        {
            roots.push(contents.join("Resources/plugins"));
        }
        roots.push(parent.join("plugins"));
    }
    if !roots.contains(&development) {
        roots.push(development);
    }
    roots
}

/// Retain the Windows installer mutex until the last GUI process exits.
/// This guard signals installation safety, and does not impose a single-instance application policy.
pub(crate) struct InstallationGuard {
    #[cfg(target_os = "windows")]
    handle: *mut std::ffi::c_void,
}

/// Mark the GUI as running, returning the native OS error if Windows cannot create its mutex.
/// CLI SDK/export commands deliberately do not acquire this guard. Other platforms need no mutex.
pub(crate) fn installation_guard() -> std::io::Result<InstallationGuard> {
    #[cfg(target_os = "windows")]
    {
        windows::guard("Local\\Nanobug.Installer.Running")
    }
    #[cfg(not(target_os = "windows"))]
    {
        Ok(InstallationGuard {})
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use super::InstallationGuard;
    use std::ffi::c_void;

    // Kernel object handles are process-owned. Keeping this tiny binding local avoids a new dependency.
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateMutexW(attributes: *const c_void, owner: i32, name: *const u16) -> *mut c_void;
        fn CloseHandle(handle: *mut c_void) -> i32;
        #[cfg(test)]
        fn OpenMutexW(access: u32, inherit: i32, name: *const u16) -> *mut c_void;
    }

    /// Open or create a named mutex without taking ownership; its existence is the installer signal.
    pub(super) fn guard(name: &str) -> std::io::Result<InstallationGuard> {
        let wide: Vec<_> = name.encode_utf16().chain([0]).collect();
        // The string is NUL-terminated and lives across the call; null attributes request OS defaults.
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, wide.as_ptr()) };
        if handle.is_null() {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(InstallationGuard { handle })
        }
    }

    impl Drop for InstallationGuard {
        fn drop(&mut self) {
            // Successful construction owns one valid handle, closed exactly once by this guard.
            unsafe { CloseHandle(self.handle) };
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// Probe with SYNCHRONIZE access and release the probe handle immediately.
        fn exists(name: &str) -> bool {
            let wide: Vec<_> = name.encode_utf16().chain([0]).collect();
            // The probe uses the same lifetime guarantees as creation and never owns the mutex.
            let handle = unsafe { OpenMutexW(0x0010_0000, 0, wide.as_ptr()) };
            if handle.is_null() {
                false
            } else {
                unsafe { CloseHandle(handle) };
                true
            }
        }

        #[test]
        fn distribution_mutex_survives_until_last_gui_guard_drops() {
            let name = format!("Local\\Nanobug.Installer.Test.{}", std::process::id());
            let first = guard(&name).unwrap();
            let second = guard(&name).unwrap();
            assert!(exists(&name));
            drop(first);
            assert!(exists(&name));
            drop(second);
            assert!(!exists(&name));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distribution_prefers_installed_resources_and_keeps_direct_launch_fallback() {
        let development = PathBuf::from("source/dist/plugins");
        assert_eq!(
            resource_roots(
                Some(Path::new("installed/Nanobug.exe")),
                development.clone()
            ),
            vec![PathBuf::from("installed/plugins"), development.clone()]
        );
        assert_eq!(resource_roots(None, development.clone()), vec![development]);
    }

    #[test]
    fn distribution_never_mixes_installed_packages_with_old_development_packages() {
        let fixture = tempfile::tempdir().unwrap();
        let installed = fixture.path().join("installed/plugins");
        let development = fixture.path().join("source/dist/plugins");
        std::fs::create_dir_all(&development).unwrap();
        let candidates = vec![installed.clone(), development.clone()];
        assert_eq!(preferred_root(candidates.clone()), vec![development]);
        std::fs::create_dir_all(&installed).unwrap();
        assert_eq!(preferred_root(candidates), vec![installed]);
    }
}
