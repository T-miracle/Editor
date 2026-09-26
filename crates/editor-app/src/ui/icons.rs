//! Resolves file and folder icons from bundled theme manifests.

use gpui_kit::{Img, Styled as _, img};
use plugin_schema::{
    FileIconConfig, FileIconRule, PluginManifest, ThemeDefinition, ThemeMode as FileThemeMode,
};
use std::{path::Path, sync::LazyLock};

static BUILTIN_RUST_PLUGIN: LazyLock<PluginManifest> = LazyLock::new(|| {
    PluginManifest::parse(include_str!("../../../../plugins/rust/plugin.toml"))
        .expect("the bundled Rust plugin manifest must be valid")
});
static BUILTIN_RUST_ICONS: LazyLock<FileIconConfig> = LazyLock::new(|| {
    assert_eq!(
        BUILTIN_RUST_PLUGIN.file_icons.as_deref(),
        Some(Path::new("icons.json")),
        "the Rust plugin must declare its file icon JSON"
    );
    FileIconConfig::parse(include_str!("../../../../plugins/rust/icons.json"))
        .expect("the bundled Rust plugin icon JSON must be valid")
});
// Keep TOML file matching and icon assets in the declarative language plugin.
static BUILTIN_TOML_PLUGIN: LazyLock<PluginManifest> = LazyLock::new(|| {
    PluginManifest::parse(include_str!("../../../../plugins/toml/plugin.toml"))
        .expect("the bundled TOML plugin manifest must be valid")
});
static BUILTIN_TOML_ICONS: LazyLock<FileIconConfig> = LazyLock::new(|| {
    assert_eq!(
        BUILTIN_TOML_PLUGIN.file_icons.as_deref(),
        Some(Path::new("icons.json")),
        "the TOML plugin must declare its file icon JSON"
    );
    FileIconConfig::parse(include_str!("../../../../plugins/toml/icons.json"))
        .expect("the bundled TOML plugin icon JSON must be valid")
});
static BUILTIN_THEME_PLUGIN: LazyLock<PluginManifest> = LazyLock::new(|| {
    PluginManifest::parse(include_str!(
        "../../../../plugins/default-light-theme/plugin.toml"
    ))
    .expect("the bundled theme plugin manifest must be valid")
});
static BUILTIN_THEME_ICONS: LazyLock<FileIconConfig> = LazyLock::new(|| {
    assert_eq!(
        BUILTIN_THEME_PLUGIN
            .theme
            .as_ref()
            .and_then(|theme| theme.file_icons.as_deref()),
        Some(Path::new("icons.json")),
        "the bundled theme must declare its file icon JSON"
    );
    FileIconConfig::parse(include_str!(
        "../../../../plugins/default-light-theme/icons.json"
    ))
    .expect("the bundled theme icon JSON must be valid")
});

const BUILTIN_RUST_PLUGIN_ASSET_ROOT: &str = "plugins/rust";
const BUILTIN_TOML_PLUGIN_ASSET_ROOT: &str = "plugins/toml";
const BUILTIN_THEME_ASSET_ROOT: &str = "plugins/default-light-theme";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileIconKind {
    Config,
    Folder,
    Image,
    Json,
    Markdown,
    Text,
    GenericFile,
}

/// Resolves an icon by theme, plugin, and system priority and renders it in full color.
///
/// The icon refuses to shrink: `flex_shrink` defaults to `1` in GPUI, so a
/// narrow explorer pane would otherwise squeeze a fixed-size icon down to
/// nothing instead of leaving it at its full size.
pub fn file_icon(path: &Path, is_folder: bool, theme: &ThemeDefinition) -> Img {
    img(icon_path(path, is_folder, theme))
        .size_4()
        .flex_shrink_0()
}

fn icon_path(path: &Path, is_folder: bool, theme: &ThemeDefinition) -> String {
    let dark = theme.mode == FileThemeMode::Dark;

    // Theme icon rules force an override before any plugin or system default.
    if let Some(icon) = BUILTIN_THEME_ICONS
        .icons
        .iter()
        .find(|icon| matches_icon(icon, path, is_folder))
    {
        return prefixed_asset_path(
            BUILTIN_THEME_ASSET_ROOT,
            if dark { &icon.dark } else { &icon.light },
        );
    }

    // The built-in Rust plugin contributes icons for Rust source and config files.
    if let Some(icon) = BUILTIN_RUST_ICONS
        .icons
        .iter()
        .find(|icon| matches_icon(icon, path, is_folder))
    {
        let asset = if dark { &icon.dark } else { &icon.light };
        return prefixed_asset_path(BUILTIN_RUST_PLUGIN_ASSET_ROOT, asset);
    }

    // The TOML plugin supplies a green T while Rust-specific TOML files keep their earlier match.
    if let Some(icon) = BUILTIN_TOML_ICONS
        .icons
        .iter()
        .find(|icon| matches_icon(icon, path, is_folder))
    {
        let asset = if dark { &icon.dark } else { &icon.light };
        return prefixed_asset_path(BUILTIN_TOML_PLUGIN_ASSET_ROOT, asset);
    }

    system_icon_path(classify_system_icon(path, is_folder), dark).to_owned()
}

/// Matches a rule by folder, exact name or extension, with path suffixes for nested config files.
fn matches_icon(rule: &FileIconRule, path: &Path, is_folder: bool) -> bool {
    // Scoped rules let language plugins recognize project configuration files.
    if !rule.project_markers.is_empty()
        && !path.ancestors().skip(1).any(|directory| {
            rule.project_markers
                .iter()
                .any(|marker| directory.join(marker).is_file())
        })
    {
        return false;
    }
    if is_folder && rule.folders {
        return true;
    }

    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if rule.files.iter().any(|selector| {
        name.eq_ignore_ascii_case(selector)
            || path
                .to_string_lossy()
                .replace('\\', "/")
                .to_ascii_lowercase()
                .ends_with(&format!("/{}", selector.to_ascii_lowercase()))
    }) {
        return true;
    }

    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            rule.extensions
                .iter()
                .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        })
}

/// Converts a validated relative asset path to the path understood by the app asset source.
fn prefixed_asset_path(root: &str, path: &Path) -> String {
    format!("{root}/{}", path.to_string_lossy().replace('\\', "/"))
}

fn classify_system_icon(path: &Path, is_folder: bool) -> FileIconKind {
    if is_folder {
        return FileIconKind::Folder;
    }

    // Extensionless and dot-prefixed configuration files need name-based fallback matching.
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if [
        "config",
        "settings",
        ".editorconfig",
        ".gitconfig",
        ".npmrc",
    ]
    .iter()
    .any(|candidate| name.eq_ignore_ascii_case(candidate))
        || name.eq_ignore_ascii_case(".env")
        || name.to_ascii_lowercase().starts_with(".env.")
    {
        return FileIconKind::Config;
    }

    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
        .unwrap_or_default()
    {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "svg" => FileIconKind::Image,
        // JSON belongs to the system icon layer unless a plugin or theme overrides it.
        "json" => FileIconKind::Json,
        "cfg" | "conf" | "config" | "ini" | "properties" | "toml" | "yaml" | "yml" => {
            FileIconKind::Config
        }
        "md" | "markdown" => FileIconKind::Markdown,
        "txt" | "text" | "log" => FileIconKind::Text,
        _ => FileIconKind::GenericFile,
    }
}

fn system_icon_path(kind: FileIconKind, dark: bool) -> &'static str {
    match kind {
        FileIconKind::Config => {
            if dark {
                "file-icons/config_dark.svg"
            } else {
                "file-icons/config.svg"
            }
        }
        FileIconKind::Folder => {
            if dark {
                "file-icons/folder_dark.svg"
            } else {
                "file-icons/folder.svg"
            }
        }
        FileIconKind::Image => {
            if dark {
                "file-icons/image_dark.svg"
            } else {
                "file-icons/image.svg"
            }
        }
        FileIconKind::Json => {
            if dark {
                "file-icons/json_dark.svg"
            } else {
                "file-icons/json.svg"
            }
        }
        FileIconKind::Markdown => {
            if dark {
                "file-icons/markdown_dark.svg"
            } else {
                "file-icons/markdown.svg"
            }
        }
        FileIconKind::Text => {
            if dark {
                "file-icons/text_dark.svg"
            } else {
                "file-icons/text.svg"
            }
        }
        FileIconKind::GenericFile => {
            if dark {
                "file-icons/file_dark.svg"
            } else {
                "file-icons/file.svg"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::file_icon;
    use gpui_kit::{Image, ImageFormat, SvgRenderer};
    use std::{collections::BTreeSet, sync::Arc};

    #[test]
    fn maps_markdown_files_and_uses_generic_file_for_other_extensions() {
        assert_eq!(
            classify_system_icon(Path::new("README.md"), false),
            FileIconKind::Markdown
        );
        assert_eq!(
            classify_system_icon(Path::new("main.ts"), false),
            FileIconKind::GenericFile
        );
    }

    #[test]
    fn maps_every_folder_to_the_generic_folder_icon() {
        assert_eq!(
            classify_system_icon(Path::new(".github"), true),
            FileIconKind::Folder
        );
        assert_eq!(
            classify_system_icon(Path::new("tests"), true),
            FileIconKind::Folder
        );
    }

    #[test]
    fn svg_image_rendering_preserves_multiple_original_colors() {
        let path = system_icon_path(FileIconKind::Markdown, true);
        let bytes = file_icon(path).expect("Markdown icon should be embedded");
        let image = Image::from_bytes(ImageFormat::Svg, bytes.to_vec());
        let rendered = image
            .to_image_data(SvgRenderer::new(Arc::new(())))
            .expect("SVG should render as a color image");
        let colors = rendered
            .as_bytes(0)
            .expect("rendered image should contain a frame")
            .chunks_exact(4)
            .filter(|pixel| pixel[3] > 0)
            .map(|pixel| [pixel[0], pixel[1], pixel[2]])
            .collect::<BTreeSet<_>>();

        assert!(colors.len() > 1, "SVG was flattened to a single color");
    }
}
