//! Resolves file and folder icons from installed plugin packages.

use gpui_kit::{Img, Styled as _, img};
use plugin_schema::{FileIconRule, ThemeDefinition, ThemeMode as FileThemeMode};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileIconKind {
    Config,
    Folder,
    Json,
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

    // The catalog orders theme overrides before language icons from installed packages.
    for (root, config, _) in crate::extensions::contributions::icon_rules().iter() {
        if let Some(icon) = config
            .icons
            .iter()
            .find(|icon| matches_icon(icon, path, is_folder))
        {
            return prefixed_asset_path(&root, if dark { &icon.dark } else { &icon.light });
        }
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
        // JSON belongs to the system icon layer unless a plugin or theme overrides it.
        "json" => FileIconKind::Json,
        "cfg" | "conf" | "config" | "ini" | "properties" | "toml" | "yaml" | "yml" => {
            FileIconKind::Config
        }
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
        FileIconKind::Json => {
            if dark {
                "file-icons/json_dark.svg"
            } else {
                "file-icons/json.svg"
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
    fn unrecognized_files_use_generic_artwork_without_language_fallbacks() {
        assert_eq!(
            classify_system_icon(Path::new("README.md"), false),
            FileIconKind::GenericFile
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
        let path = system_icon_path(FileIconKind::Folder, true);
        let bytes = file_icon(path).expect("Generic folder icon should be embedded");
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
