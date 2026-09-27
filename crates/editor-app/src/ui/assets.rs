//! Embeds file icons and exposes them to GPUI's asset loader.

use gpui_kit::{AssetSource, Result, SharedString, assets::Assets};
use std::borrow::Cow;

macro_rules! file_icons {
    ($($path:literal => $file:literal),+ $(,)?) => {
        const FILE_ICON_PATHS: &[&str] = &[$($path),+];

        // Embed bundled system and plugin icons for synchronous asset resolution.
        pub(crate) fn file_icon(path: &str) -> Option<&'static [u8]> {
            match path {
                $($path => Some(include_bytes!($file).as_slice()),)+
                _ => None,
            }
        }
    };
}

file_icons! {
    "file-icons/config.svg" => "../../assets/file-icons/config.svg",
    "file-icons/config_dark.svg" => "../../assets/file-icons/config_dark.svg",
    "file-icons/file.svg" => "../../assets/file-icons/file.svg",
    "file-icons/file_dark.svg" => "../../assets/file-icons/file_dark.svg",
    "file-icons/folder.svg" => "../../assets/file-icons/folder.svg",
    "file-icons/folder_dark.svg" => "../../assets/file-icons/folder_dark.svg",
    "file-icons/image.svg" => "../../assets/file-icons/image.svg",
    "file-icons/image_dark.svg" => "../../assets/file-icons/image_dark.svg",
    "file-icons/json.svg" => "../../assets/file-icons/json.svg",
    "file-icons/json_dark.svg" => "../../assets/file-icons/json_dark.svg",
    "file-icons/markdown.svg" => "../../assets/file-icons/markdown.svg",
    "file-icons/markdown_dark.svg" => "../../assets/file-icons/markdown_dark.svg",
    "file-icons/text.svg" => "../../assets/file-icons/text.svg",
    "file-icons/text_dark.svg" => "../../assets/file-icons/text_dark.svg",
}

/// Combines GPUI Kit's monochrome UI icons with the editor's file icons.
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(icon) = file_icon(path) {
            return Ok(Some(Cow::Borrowed(icon)));
        }
        if let Some(icon) = crate::extensions::contributions::asset(path) {
            return Ok(Some(Cow::Owned(icon)));
        }
        Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = Assets.list(path)?;
        paths.extend(
            FILE_ICON_PATHS
                .iter()
                .filter(|candidate| candidate.starts_with(path))
                .map(|candidate| SharedString::from(*candidate)),
        );
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_every_embedded_file_icon() {
        for path in FILE_ICON_PATHS {
            assert!(file_icon(path).is_some(), "missing embedded icon: {path}");
        }
    }
}
