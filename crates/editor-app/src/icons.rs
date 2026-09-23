use gpui_kit::{Img, Styled as _, img};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileIconKind {
    Folder,
    GithubFolder,
    TestFolder,
    Rust,
    TypeScript,
    Tsx,
    JavaScript,
    Jsx,
    Vue,
    Html,
    Css,
    Json,
    Toml,
    Markdown,
    Yaml,
    Cargo,
    CargoLock,
    GitIgnore,
    Text,
}

/// Resolves a JetBrains 2023 file icon and renders it as a full-color image.
///
/// The icon refuses to shrink: `flex_shrink` defaults to `1` in GPUI, so a
/// narrow explorer pane would otherwise squeeze a fixed-size icon down to
/// nothing instead of leaving it at its full size.
pub fn file_icon(path: &Path, is_folder: bool, dark: bool) -> Img {
    img(icon_path(classify(path, is_folder), dark))
        .size_4()
        .flex_shrink_0()
}

fn classify(path: &Path, is_folder: bool) -> FileIconKind {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();

    if is_folder {
        return match name.to_ascii_lowercase().as_str() {
            ".github" => FileIconKind::GithubFolder,
            "test" | "tests" | "spec" | "specs" | "__test__" | "__tests__" => {
                FileIconKind::TestFolder
            }
            _ => FileIconKind::Folder,
        };
    }

    match name {
        "Cargo.toml" => return FileIconKind::Cargo,
        "Cargo.lock" => return FileIconKind::CargoLock,
        ".gitignore" => return FileIconKind::GitIgnore,
        _ => {}
    }

    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
        .unwrap_or_default()
    {
        "rs" => FileIconKind::Rust,
        "ts" => FileIconKind::TypeScript,
        "tsx" => FileIconKind::Tsx,
        "js" => FileIconKind::JavaScript,
        "jsx" => FileIconKind::Jsx,
        "vue" => FileIconKind::Vue,
        "html" | "htm" => FileIconKind::Html,
        "css" => FileIconKind::Css,
        "json" => FileIconKind::Json,
        "toml" => FileIconKind::Toml,
        "md" | "markdown" => FileIconKind::Markdown,
        "yaml" | "yml" => FileIconKind::Yaml,
        _ => FileIconKind::Text,
    }
}

macro_rules! themed_icon_path {
    ($dark:expr, $light_file:literal, $dark_file:literal) => {
        if $dark {
            concat!("jetbrains-2023/", $dark_file)
        } else {
            concat!("jetbrains-2023/", $light_file)
        }
    };
}

fn icon_path(kind: FileIconKind, dark: bool) -> &'static str {
    match kind {
        FileIconKind::Folder => themed_icon_path!(dark, "folder.svg", "folder_dark.svg"),
        FileIconKind::GithubFolder => {
            themed_icon_path!(dark, "folderGithub.svg", "folderGithub_dark.svg")
        }
        FileIconKind::TestFolder => {
            themed_icon_path!(dark, "folderTest.svg", "folderTest_dark.svg")
        }
        FileIconKind::Rust => themed_icon_path!(dark, "rustFile.svg", "rustFile_dark.svg"),
        FileIconKind::TypeScript => {
            themed_icon_path!(dark, "typeScript.svg", "typeScript_dark.svg")
        }
        FileIconKind::Tsx => themed_icon_path!(dark, "tsx.svg", "tsx_dark.svg"),
        FileIconKind::JavaScript => {
            themed_icon_path!(dark, "javaScript.svg", "javaScript_dark.svg")
        }
        FileIconKind::Jsx => themed_icon_path!(dark, "jsx.svg", "jsx_dark.svg"),
        FileIconKind::Vue => "jetbrains-2023/vueJs.svg",
        FileIconKind::Html => themed_icon_path!(dark, "html.svg", "html_dark.svg"),
        FileIconKind::Css => "jetbrains-2023/css.svg",
        FileIconKind::Json => themed_icon_path!(dark, "json.svg", "json_dark.svg"),
        FileIconKind::Toml => themed_icon_path!(dark, "toml.svg", "toml_dark.svg"),
        FileIconKind::Markdown => themed_icon_path!(dark, "markdown.svg", "markdown_dark.svg"),
        FileIconKind::Yaml => themed_icon_path!(dark, "yaml.svg", "yaml_dark.svg"),
        FileIconKind::Cargo => themed_icon_path!(dark, "cargo.svg", "cargo_dark.svg"),
        FileIconKind::CargoLock => themed_icon_path!(dark, "cargoLock.svg", "cargoLock_dark.svg"),
        FileIconKind::GitIgnore => "jetbrains-2023/gitignore.svg",
        FileIconKind::Text => themed_icon_path!(dark, "text.svg", "text_dark.svg"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::jetbrains_icon;
    use gpui_kit::{Image, ImageFormat, SvgRenderer};
    use std::{collections::BTreeSet, sync::Arc};

    #[test]
    fn maps_special_files_before_extensions() {
        assert_eq!(
            classify(Path::new("Cargo.toml"), false),
            FileIconKind::Cargo
        );
        assert_eq!(
            classify(Path::new("Cargo.lock"), false),
            FileIconKind::CargoLock
        );
        assert_eq!(
            classify(Path::new("README.md"), false),
            FileIconKind::Markdown
        );
    }

    #[test]
    fn maps_special_folders() {
        assert_eq!(
            classify(Path::new(".github"), true),
            FileIconKind::GithubFolder
        );
        assert_eq!(classify(Path::new("tests"), true), FileIconKind::TestFolder);
    }

    #[test]
    fn svg_image_rendering_preserves_multiple_original_colors() {
        let path = icon_path(FileIconKind::JavaScript, true);
        let bytes = jetbrains_icon(path).expect("JavaScript icon should be embedded");
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
