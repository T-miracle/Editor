//! Embeds the product mark and file icons for GPUI's asset loader.

use gpui_kit::{AssetSource, Result, SharedString, assets::Assets};
use std::borrow::Cow;

/// Asset path of the supplied product mark, rendered in full color in the native title bar.
pub(crate) const APP_ICON_PATH: &str = "branding/nanobug.png";

// The default bundle omits these configuration and shortcut actions; embed their catalog SVGs.
gpui_kit::assets::icon_assets!(ConfigurationIcons, [Lock, SquarePen, Keyboard, RotateCcw]);

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
    // The title bar, repository READMEs, and Windows ICO share the same supplied artwork.
    "branding/nanobug.png" => "../../assets/branding/nanobug.png",
    // Run actions use editor-owned monochrome geometry, including filled Play/Stop treatment.
    "icons/run-build.svg" => "../../assets/icons/run-build.svg",
    "icons/run-start.svg" => "../../assets/icons/run-start.svg",
    "icons/run-debug.svg" => "../../assets/icons/run-debug.svg",
    "icons/run-stop.svg" => "../../assets/icons/run-stop.svg",
    "icons/run-chevron-down.svg" => "../../assets/icons/run-chevron-down.svg",
    // Custom explorer actions share the title's theme color through monochrome SVG rendering.
    "icons/explorer-locate.svg" => "../../assets/icons/explorer-locate.svg",
    "icons/explorer-collapse-all.svg" => "../../assets/icons/explorer-collapse-all.svg",
    "icons/explorer-expand-all.svg" => "../../assets/icons/explorer-expand-all.svg",
    // The supplied outline artwork shares the window control group's theme color.
    "icons/outline-panel.svg" => "../../assets/icons/outline-panel.svg",
    "file-icons/config.svg" => "../../assets/file-icons/config.svg",
    "file-icons/config_dark.svg" => "../../assets/file-icons/config_dark.svg",
    "file-icons/file.svg" => "../../assets/file-icons/file.svg",
    "file-icons/file_dark.svg" => "../../assets/file-icons/file_dark.svg",
    "file-icons/folder.svg" => "../../assets/file-icons/folder.svg",
    "file-icons/folder_dark.svg" => "../../assets/file-icons/folder_dark.svg",
    "file-icons/json.svg" => "../../assets/file-icons/json.svg",
    "file-icons/json_dark.svg" => "../../assets/file-icons/json_dark.svg",
    "file-icons/text.svg" => "../../assets/file-icons/text.svg",
    "file-icons/text_dark.svg" => "../../assets/file-icons/text_dark.svg",
}

/// Combines the product mark, editor file icons, and GPUI Kit's monochrome UI icons.
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(icon) = file_icon(path) {
            return Ok(Some(Cow::Borrowed(icon)));
        }
        if let Some(icon) = crate::extensions::contributions::asset(path) {
            return Ok(Some(Cow::Owned(icon)));
        }
        if let Some(icon) = ConfigurationIcons.load(path)? {
            return Ok(Some(icon));
        }
        Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = Assets.list(path)?;
        paths.extend(ConfigurationIcons.list(path)?);
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

    /// Every header action must resolve its SVG from the application bundle in either theme.
    #[test]
    fn exposes_explorer_title_icons() {
        for path in [
            "icons/explorer-locate.svg",
            "icons/explorer-collapse-all.svg",
            "icons/explorer-expand-all.svg",
        ] {
            assert!(AppAssets.load(path).unwrap().is_some());
            assert!(
                AppAssets
                    .list("icons/")
                    .unwrap()
                    .iter()
                    .any(|entry| entry == path)
            );
        }
    }

    /// Icon-only execution controls must resolve every asset in the application bundle.
    #[test]
    fn exposes_run_title_icons() {
        for path in [
            "icons/run-build.svg",
            "icons/run-start.svg",
            "icons/run-debug.svg",
            "icons/run-stop.svg",
        ] {
            assert!(
                AppAssets.load(path).unwrap().is_some(),
                "missing run action icon: {path}"
            );
        }
    }

    /// Configuration and shortcut controls resolve from the selected extra icon bundle.
    #[test]
    fn exposes_configuration_action_icons() {
        for icon in [
            gpui_kit::assets::IconName::Lock,
            gpui_kit::assets::IconName::SquarePen,
            gpui_kit::assets::IconName::Keyboard,
            gpui_kit::assets::IconName::RotateCcw,
        ] {
            let path = icon.path();
            assert!(AppAssets.load(&path).unwrap().is_some());
            assert!(
                AppAssets
                    .list("icons/")
                    .unwrap()
                    .iter()
                    .any(|entry| entry == &path)
            );
        }
    }
}
