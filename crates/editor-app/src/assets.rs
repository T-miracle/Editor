use gpui_kit::{AssetSource, Result, SharedString, assets::Assets};
use std::borrow::Cow;

macro_rules! jetbrains_icons {
    ($($path:literal => $file:literal),+ $(,)?) => {
        const JETBRAINS_ICON_PATHS: &[&str] = &[$($path),+];

        pub(crate) fn jetbrains_icon(path: &str) -> Option<&'static [u8]> {
            match path {
                $($path => Some(include_bytes!(concat!("../assets/jetbrains-2023/", $file)).as_slice()),)+
                _ => None,
            }
        }
    };
}

jetbrains_icons! {
    "jetbrains-2023/cargo.svg" => "cargo.svg",
    "jetbrains-2023/cargo_dark.svg" => "cargo_dark.svg",
    "jetbrains-2023/cargoLock.svg" => "cargoLock.svg",
    "jetbrains-2023/cargoLock_dark.svg" => "cargoLock_dark.svg",
    "jetbrains-2023/css.svg" => "css.svg",
    "jetbrains-2023/folder.svg" => "folder.svg",
    "jetbrains-2023/folder_dark.svg" => "folder_dark.svg",
    "jetbrains-2023/folderGithub.svg" => "folderGithub.svg",
    "jetbrains-2023/folderGithub_dark.svg" => "folderGithub_dark.svg",
    "jetbrains-2023/folderTest.svg" => "folderTest.svg",
    "jetbrains-2023/folderTest_dark.svg" => "folderTest_dark.svg",
    "jetbrains-2023/gitignore.svg" => "gitignore.svg",
    "jetbrains-2023/html.svg" => "html.svg",
    "jetbrains-2023/html_dark.svg" => "html_dark.svg",
    "jetbrains-2023/javaScript.svg" => "javaScript.svg",
    "jetbrains-2023/javaScript_dark.svg" => "javaScript_dark.svg",
    "jetbrains-2023/json.svg" => "json.svg",
    "jetbrains-2023/json_dark.svg" => "json_dark.svg",
    "jetbrains-2023/jsx.svg" => "jsx.svg",
    "jetbrains-2023/jsx_dark.svg" => "jsx_dark.svg",
    "jetbrains-2023/markdown.svg" => "markdown.svg",
    "jetbrains-2023/markdown_dark.svg" => "markdown_dark.svg",
    "jetbrains-2023/rustFile.svg" => "rustFile.svg",
    "jetbrains-2023/rustFile_dark.svg" => "rustFile_dark.svg",
    "jetbrains-2023/text.svg" => "text.svg",
    "jetbrains-2023/text_dark.svg" => "text_dark.svg",
    "jetbrains-2023/toml.svg" => "toml.svg",
    "jetbrains-2023/toml_dark.svg" => "toml_dark.svg",
    "jetbrains-2023/tsx.svg" => "tsx.svg",
    "jetbrains-2023/tsx_dark.svg" => "tsx_dark.svg",
    "jetbrains-2023/typeScript.svg" => "typeScript.svg",
    "jetbrains-2023/typeScript_dark.svg" => "typeScript_dark.svg",
    "jetbrains-2023/vueJs.svg" => "vueJs.svg",
    "jetbrains-2023/yaml.svg" => "yaml.svg",
    "jetbrains-2023/yaml_dark.svg" => "yaml_dark.svg",
}

/// Combines GPUI Kit's monochrome UI icons with the original full-color file icons.
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(icon) = jetbrains_icon(path) {
            return Ok(Some(Cow::Borrowed(icon)));
        }
        Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = Assets.list(path)?;
        paths.extend(
            JETBRAINS_ICON_PATHS
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
    fn exposes_every_embedded_jetbrains_icon() {
        for path in JETBRAINS_ICON_PATHS {
            assert!(
                jetbrains_icon(path).is_some(),
                "missing embedded icon: {path}"
            );
        }
    }
}
