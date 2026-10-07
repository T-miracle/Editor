//! File clipboard offers preserve native move intent instead of treating every path list as a copy.

use super::Kind;
use gpui_kit::{App, ClipboardEntry, ClipboardItem};
use rust_i18n::t;
use std::path::PathBuf;

/// Read GPUI file offers and file-URI lists; ordinary text never becomes a disk operation.
pub(crate) fn read(item: Option<&ClipboardItem>) -> Result<(Vec<PathBuf>, Kind), String> {
    let item = item.ok_or_else(|| t!("explorer.no_clipboard_files").to_string())?;
    let paths = item
        .entries()
        .iter()
        .filter_map(|entry| {
            if let ClipboardEntry::ExternalPaths(paths) = entry {
                Some(paths.paths().iter().cloned())
            } else {
                None
            }
        })
        .flatten()
        .collect::<Vec<_>>();
    if !paths.is_empty() {
        #[cfg(windows)]
        let kind = windows::intent(&paths).unwrap_or(Kind::Copy);
        #[cfg(not(windows))]
        let kind = Kind::Copy;
        return Ok((paths, kind));
    }
    let text = item.text().unwrap_or_default();
    let mut lines = text
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .peekable();
    let kind = match lines.peek().copied() {
        Some("cut") => {
            lines.next();
            Kind::Move
        }
        Some("copy") => {
            lines.next();
            Kind::Copy
        }
        Some(line) if line.starts_with("file://") => Kind::Copy,
        _ => return Err(t!("explorer.no_clipboard_files").to_string()),
    };
    let paths = lines
        .map(|line| {
            url::Url::parse(line)
                .ok()
                .filter(|uri| uri.scheme() == "file")
                .and_then(|uri| uri.to_file_path().ok())
                .ok_or_else(|| t!("explorer.invalid_source").to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    if paths.is_empty() {
        return Err(t!("explorer.no_clipboard_files").to_string());
    }
    Ok((paths, kind))
}

/// Write native Windows file formats or portable file URI offers without shelling out.
pub(crate) fn write(paths: &[PathBuf], kind: Kind, cx: &mut App) -> Result<(), String> {
    #[cfg(windows)]
    {
        let _ = cx;
        windows::write(paths, kind)
    }
    #[cfg(not(windows))]
    {
        let mut text = if kind == Kind::Move { "cut" } else { "copy" }.to_string();
        for path in paths {
            let uri = url::Url::from_file_path(path)
                .map_err(|_| t!("explorer.invalid_source").to_string())?;
            text.push('\n');
            text.push_str(uri.as_str());
        }
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        Ok(())
    }
}

/// The sequence token protects a newer clipboard offer from a completed cut's cleanup.
pub(crate) fn sequence() -> Option<u32> {
    #[cfg(windows)]
    {
        Some(windows::sequence())
    }
    #[cfg(not(windows))]
    None
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::{
        ffi::OsString,
        os::windows::ffi::{OsStrExt as _, OsStringExt as _},
        ptr,
    };
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetForegroundWindow() -> *mut ();
        fn OpenClipboard(window: *mut ()) -> i32;
        fn CloseClipboard() -> i32;
        fn EmptyClipboard() -> i32;
        fn GetClipboardData(format: u32) -> *mut ();
        fn SetClipboardData(format: u32, memory: *mut ()) -> *mut ();
        fn RegisterClipboardFormatW(name: *const u16) -> u32;
        fn GetClipboardSequenceNumber() -> u32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalAlloc(flags: u32, size: usize) -> *mut ();
        fn GlobalLock(memory: *mut ()) -> *mut u8;
        fn GlobalUnlock(memory: *mut ()) -> i32;
        fn GlobalFree(memory: *mut ()) -> *mut ();
        fn GlobalSize(memory: *mut ()) -> usize;
    }
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn DragQueryFileW(drop: *mut (), index: u32, path: *mut u16, length: u32) -> u32;
    }

    struct Guard;
    impl Guard {
        fn open() -> Option<Self> {
            (unsafe { OpenClipboard(GetForegroundWindow()) } != 0).then_some(Self)
        }
    }
    impl Drop for Guard {
        fn drop(&mut self) {
            unsafe {
                CloseClipboard();
            }
        }
    }
    pub(super) fn sequence() -> u32 {
        unsafe { GetClipboardSequenceNumber() }
    }

    fn preferred_format() -> u32 {
        let name: Vec<u16> = "Preferred DropEffect".encode_utf16().chain([0]).collect();
        unsafe { RegisterClipboardFormatW(name.as_ptr()) }
    }

    /// Compare every native path under the clipboard lock before trusting its move preference.
    pub(super) fn intent(paths: &[PathBuf]) -> Option<Kind> {
        let _guard = Guard::open()?;
        let drop = unsafe { GetClipboardData(15) };
        if drop.is_null() {
            return None;
        }
        let count = unsafe { DragQueryFileW(drop, u32::MAX, ptr::null_mut(), 0) };
        if count as usize != paths.len() {
            return None;
        }
        for (index, expected) in paths.iter().enumerate() {
            let length = unsafe { DragQueryFileW(drop, index as u32, ptr::null_mut(), 0) };
            let mut text = vec![0u16; length as usize + 1];
            unsafe {
                DragQueryFileW(drop, index as u32, text.as_mut_ptr(), text.len() as u32);
            }
            let actual = PathBuf::from(OsString::from_wide(&text[..length as usize]));
            if actual.canonicalize().ok()? != expected.canonicalize().ok()? {
                return None;
            }
        }
        let memory = unsafe { GetClipboardData(preferred_format()) };
        if memory.is_null() || unsafe { GlobalSize(memory) } < 4 {
            return Some(Kind::Copy);
        }
        let bytes = unsafe { GlobalLock(memory) };
        if bytes.is_null() {
            return None;
        }
        let effect = unsafe { ptr::read_unaligned(bytes.cast::<u32>()) };
        unsafe {
            GlobalUnlock(memory);
        }
        Some(if effect & 2 != 0 {
            Kind::Move
        } else {
            Kind::Copy
        })
    }

    /// Global memory ownership transfers to Windows only after SetClipboardData succeeds.
    fn publish(format: u32, bytes: &[u8]) -> Result<(), String> {
        unsafe {
            let memory = GlobalAlloc(0x0042, bytes.len());
            if memory.is_null() {
                return Err(t!("explorer.clipboard_failed").to_string());
            }
            let data = GlobalLock(memory);
            if data.is_null() {
                GlobalFree(memory);
                return Err(t!("explorer.clipboard_failed").to_string());
            }
            ptr::copy_nonoverlapping(bytes.as_ptr(), data, bytes.len());
            GlobalUnlock(memory);
            if SetClipboardData(format, memory).is_null() {
                GlobalFree(memory);
                return Err(t!("explorer.clipboard_failed").to_string());
            }
        }
        Ok(())
    }

    pub(super) fn write(paths: &[PathBuf], kind: Kind) -> Result<(), String> {
        if paths.is_empty() || paths.iter().any(|path| !path.exists()) {
            return Err(t!("explorer.invalid_source").to_string());
        }
        let mut wide = Vec::new();
        for path in paths {
            wide.extend(path.as_os_str().encode_wide());
            wide.push(0);
        }
        wide.push(0);
        let mut bytes = vec![0u8; 20];
        bytes[..4].copy_from_slice(&20u32.to_le_bytes());
        bytes[16..20].copy_from_slice(&1u32.to_le_bytes());
        for unit in wide {
            bytes.extend(unit.to_le_bytes());
        }
        // EmptyClipboard needs an owner window before publishing handles.
        let _guard = Guard::open().ok_or_else(|| t!("explorer.clipboard_failed").to_string())?;
        if unsafe { EmptyClipboard() } == 0 {
            return Err(t!("explorer.clipboard_failed").to_string());
        }
        publish(15, &bytes)?;
        publish(
            preferred_format(),
            &(if kind == Kind::Move { 2u32 } else { 1u32 }).to_le_bytes(),
        )
    }
}
