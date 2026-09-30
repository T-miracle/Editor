//! Declarative window materials and host color syntax, independent of GPUI.

use serde::{Deserialize, Serialize};

/// Native window effects remain opaque when an older theme omits this section.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct ThemeWindow {
    /// Controls the OS backdrop; element colors independently control transparency.
    pub background_appearance: ThemeWindowBackground,
}

/// These modes map to GPUI's portable native window background options.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeWindowBackground {
    #[default]
    Opaque,
    Transparent,
    /// Blur applies behind the entire native window, subject to OS support.
    Blurred,
}

/// Host colors accept a trailing alpha byte; runtime plugin tokens remain RGB.
pub(crate) fn is_theme_color(value: &str) -> bool {
    matches!(value.len(), 7 | 9)
        && value.starts_with('#')
        && value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ThemeComponent, ThemeFile, ThemeFileError};

    /// The reference stylesheet explicitly includes optional fields and all state blocks.
    #[test]
    fn default_stylesheet_has_no_implicit_serialized_fields() {
        // Compare field paths so JSON's 14 and Rust's serialized 14.0 are equivalent.
        fn field_paths(
            value: &serde_json::Value,
            prefix: &str,
            fields: &mut std::collections::BTreeSet<String>,
        ) {
            match value {
                serde_json::Value::Object(object) => {
                    for (name, value) in object {
                        let path = format!("{prefix}.{name}");
                        fields.insert(path.clone());
                        field_paths(value, &path, fields);
                    }
                }
                serde_json::Value::Array(array) => {
                    for (index, value) in array.iter().enumerate() {
                        field_paths(value, &format!("{prefix}[{index}]"), fields);
                    }
                }
                _ => {}
            }
        }
        let source = include_str!("../../editor-app/assets/themes/default.json");
        let file = ThemeFile::parse(source).unwrap();
        let mut declared = std::collections::BTreeSet::new();
        let mut complete = std::collections::BTreeSet::new();
        field_paths(&serde_json::from_str(source).unwrap(), "", &mut declared);
        field_paths(&serde_json::to_value(&file).unwrap(), "", &mut complete);
        assert_eq!(declared, complete);
        for theme in file.themes {
            assert_eq!(theme.components.len(), 17);
            for styles in theme.components.values() {
                assert!(
                    styles.hover.is_some() && styles.selected.is_some() && styles.active.is_some()
                );
            }
        }
    }

    /// Old files stay opaque, while new materials survive parse and serialization.
    #[test]
    fn window_materials_are_optional_and_validated() {
        let mut source: serde_json::Value =
            serde_json::from_str(include_str!("../../editor-app/assets/themes/default.json"))
                .unwrap();
        source["themes"][0]
            .as_object_mut()
            .unwrap()
            .remove("window");
        assert_eq!(
            ThemeFile::parse(&source.to_string()).unwrap().themes[0].window,
            ThemeWindow::default()
        );
        for mode in ["opaque", "transparent", "blurred"] {
            source["themes"][0]["window"] = serde_json::json!({"background_appearance": mode});
            let file = ThemeFile::parse(&source.to_string()).unwrap();
            assert_eq!(
                serde_json::to_value(&file).unwrap()["themes"][0]["window"]["background_appearance"],
                mode
            );
        }
        source["themes"][0]["window"]["background_appearance"] = "unknown".into();
        assert!(ThemeFile::parse(&source.to_string()).is_err());
        source["themes"][0]["window"] = serde_json::json!({"blur_radius": 10});
        assert!(ThemeFile::parse(&source.to_string()).is_err());
    }

    /// Accept both RGB and RGBA for host styles without widening the guest RGB contract.
    #[test]
    fn host_alpha_colors_are_validated_at_every_style_state() {
        let mut file =
            ThemeFile::parse(include_str!("../../editor-app/assets/themes/default.json")).unwrap();
        file.themes[0].colors.background = "#ffffff80".into();
        let styles = file.themes[0]
            .components
            .get_mut(&ThemeComponent::ExplorerRow)
            .unwrap();
        styles.base.background = Some("#12345600".into());
        styles.hover.as_mut().unwrap().background = Some("#12345680".into());
        styles.selected.as_mut().unwrap().background = Some("#123456ff".into());
        styles.active.as_mut().unwrap().background = Some("#abcdefCC".into());
        file.validate().unwrap();
        for invalid in ["#fff", "#1234567", "#123456gg", "12345678", "#你好"] {
            file.themes[0].colors.background = invalid.into();
            assert!(matches!(
                file.validate(),
                Err(ThemeFileError::InvalidColor(path, _)) if path == "colors.background"
            ));
        }
        file.themes[0].colors.background = "#ffffff80".into();
        file.themes[0]
            .legacy_plugin_colors
            .insert("example.background".into(), "#12345680".into());
        assert!(matches!(
            file.validate(),
            Err(ThemeFileError::InvalidColor(path, _)) if path == "plugin_colors.example.background"
        ));
    }
}
