//! Loads theme definitions and applies their colors to GPUI.

use std::{
    collections::BTreeMap,
    sync::{Arc, LazyLock},
};

mod window;
pub use window::window_background;

use gpui_kit::{
    App, Global, Hsla,
    component::{Theme, ThemeMode},
    px, rgb, rgba,
};
use plugin_schema::{
    ComponentStyles, PluginTextStyle, StyleProperties, ThemeComponent, ThemeDefinition, ThemeFile,
    ThemeMode as FileThemeMode, ThemeTypography, ThemeWindow,
};

// Both palettes ship with the editor so first launch does not depend on plugin installation.
static BUILTIN_THEME_FILE: LazyLock<ThemeFile> = LazyLock::new(|| {
    ThemeFile::parse(include_str!("../../assets/themes/default.json"))
        .expect("the bundled theme file must satisfy the plugin schema")
});

#[derive(Clone, Copy, Default)]
pub struct ResolvedStyle {
    pub background: Option<Hsla>,
    pub foreground: Option<Hsla>,
    pub border: Option<Hsla>,
    pub radius_px: Option<f32>,
    pub font_size_px: Option<f32>,
    pub padding_x_px: Option<f32>,
    pub padding_y_px: Option<f32>,
}

#[derive(Clone, Copy, Default)]
pub struct ResolvedComponentStyles {
    pub base: ResolvedStyle,
    pub hover: ResolvedStyle,
    pub selected: ResolvedStyle,
    pub active: ResolvedStyle,
}

#[derive(Default)]
struct RuntimeStyles {
    components: BTreeMap<ThemeComponent, ResolvedComponentStyles>,
    plugin_colors: BTreeMap<String, u32>,
    plugin_text_styles: BTreeMap<String, PluginTextStyle>,
    typography: ThemeTypography,
    window: ThemeWindow,
}

impl Global for RuntimeStyles {}

pub fn builtin_theme(dark: bool) -> &'static ThemeDefinition {
    let mode = if dark {
        FileThemeMode::Dark
    } else {
        FileThemeMode::Light
    };
    BUILTIN_THEME_FILE
        .themes
        .iter()
        .find(|theme| theme.mode == mode)
        .expect("the bundled theme file must contain light and dark themes")
}

pub fn component_styles(cx: &App, component: ThemeComponent) -> ResolvedComponentStyles {
    cx.global::<RuntimeStyles>()
        .components
        .get(&component)
        .copied()
        .unwrap_or_default()
}

pub fn typography(cx: &App) -> ThemeTypography {
    cx.global::<RuntimeStyles>().typography.clone()
}

pub fn apply_theme(theme: &ThemeDefinition, cx: &mut App) {
    let dark = theme.mode == FileThemeMode::Dark;
    Theme::change(
        if dark {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        },
        None,
        cx,
    );

    let palette = &theme.colors;
    let theme_typography = &theme.typography;
    let background = color(&palette.background);
    let surface = color(&palette.surface);
    let hover = color(&palette.hover);
    let border = color(&palette.border);
    let foreground = color(&palette.foreground);
    let muted_foreground = color(&palette.muted_foreground);
    let selection = color(&palette.selection);
    let accent = color(&palette.accent);
    let accent_hover = color(&palette.accent_hover);
    let accent_active = color(&palette.accent_active);
    let editor_background = theme
        .components
        .get(&ThemeComponent::Editor)
        .and_then(|styles| styles.base.background.as_deref())
        .map(color)
        .unwrap_or(background);
    let explorer_row = theme.components.get(&ThemeComponent::ExplorerRow);
    let row_hover = explorer_row
        .and_then(|styles| styles.hover.as_ref())
        .and_then(|styles| styles.background.as_deref())
        .map(color)
        .unwrap_or(hover);
    let row_selected = explorer_row
        .and_then(|styles| styles.selected.as_ref())
        .and_then(|styles| styles.background.as_deref())
        .map(color)
        .unwrap_or(selection);
    let row_selected_border = explorer_row
        .and_then(|styles| styles.selected.as_ref())
        .and_then(|styles| styles.border.as_deref())
        .map(color)
        .unwrap_or(accent);
    let title_bar = theme.components.get(&ThemeComponent::WindowTitleBar);

    // Update legacy colors, renderable tokens and the Base projection together;
    // otherwise Root would retain the component library's opaque background.
    Theme::update(cx, |theme| {
        theme.background = background;
        theme.foreground = foreground;
        theme.border = border;
        theme.sidebar = surface;
        theme.sidebar_foreground = foreground;
        theme.sidebar_border = border;
        theme.sidebar_accent = hover;
        theme.sidebar_accent_foreground = foreground;
        theme.tab_bar = surface;
        theme.tab = surface;
        theme.tab_foreground = muted_foreground;
        theme.tab_active = background;
        theme.tab_active_foreground = foreground;
        theme.muted = surface;
        theme.muted_foreground = muted_foreground;
        theme.input = border;
        theme.colors.list = surface;
        theme.list_hover = row_hover;
        theme.list_active = row_selected;
        theme.list_active_border = row_selected_border;
        theme.selection = selection;
        theme.accent = hover;
        theme.accent_foreground = foreground;
        theme.primary = accent;
        theme.primary_hover = accent_hover;
        theme.primary_active = accent_active;
        theme.primary_foreground = color("#ffffff");
        theme.button = surface;
        theme.button_hover = hover;
        theme.button_active = border;
        theme.button_foreground = foreground;
        theme.button_primary = accent;
        theme.button_primary_hover = accent_hover;
        theme.button_primary_active = accent_active;
        theme.button_primary_foreground = color("#ffffff");
        theme.popover = surface;
        theme.popover_foreground = foreground;
        theme.ring = accent;
        theme.drag_border = accent;
        theme.caret = accent;
        theme.radius = px(6.);
        theme.radius_lg = px(8.);
        theme.title_bar = title_bar
            .and_then(|styles| styles.base.background.as_deref())
            .map(color)
            .unwrap_or(surface);
        theme.title_bar_border = title_bar
            .and_then(|styles| styles.base.border.as_deref())
            .map(color)
            .unwrap_or(border);
        if let Some(family) = &theme_typography.ui.family {
            theme.font_family = family.clone().into();
        }
        if let Some(family) = &theme_typography.mono.family {
            theme.mono_font_family = family.clone().into();
        }
        if editor_background.a < 1. {
            // Editor gutters and ghost-text erasure must not restore an opaque fill.
            let highlight = Arc::make_mut(&mut theme.highlight_theme);
            highlight.style.editor_background = Some(editor_background);
            highlight.style.editor_gutter_background = Some(editor_background);
        }
    });
    apply_font_sizes(cx, theme_typography);
    Theme::sync_base(cx);

    let components = theme
        .components
        .iter()
        .map(|(component, styles)| (*component, resolve_component_styles(styles)))
        .collect();
    // Theme validation guarantees #RRGGBB, so plugins receive numeric colors only.
    let plugin_colors = theme
        .plugin_colors()
        .into_iter()
        .map(|(key, value)| {
            (
                key,
                u32::from_str_radix(value.trim_start_matches('#'), 16).unwrap(),
            )
        })
        .collect();
    let plugin_text_styles = theme.plugin_text_styles();
    let typography = theme.typography.clone();
    if cx.has_global::<RuntimeStyles>() {
        let runtime = cx.global_mut::<RuntimeStyles>();
        runtime.components = components;
        runtime.plugin_colors = plugin_colors;
        runtime.plugin_text_styles = plugin_text_styles;
        runtime.typography = typography;
        runtime.window = theme.window;
    } else {
        cx.set_global(RuntimeStyles {
            components,
            plugin_colors,
            plugin_text_styles,
            typography,
            window: theme.window,
        });
    }
    // Runtime colors must be available before native editor scrollbar styles are resolved.
    super::controls::install_scrollbar_theme(cx);
    window::register(cx);
    // Re-render existing Roots so theme switches also restore an opaque window.
    cx.refresh_windows();
}

fn resolve_component_styles(styles: &ComponentStyles) -> ResolvedComponentStyles {
    ResolvedComponentStyles {
        base: resolve_style(&styles.base),
        hover: styles.hover.as_ref().map(resolve_style).unwrap_or_default(),
        selected: styles
            .selected
            .as_ref()
            .map(resolve_style)
            .unwrap_or_default(),
        active: styles
            .active
            .as_ref()
            .map(resolve_style)
            .unwrap_or_default(),
    }
}

fn resolve_style(style: &StyleProperties) -> ResolvedStyle {
    ResolvedStyle {
        background: style.background.as_deref().map(color),
        foreground: style.foreground.as_deref().map(color),
        border: style.border.as_deref().map(color),
        radius_px: style.radius_px,
        font_size_px: style.font_size_px,
        padding_x_px: style.padding_x_px,
        padding_y_px: style.padding_y_px,
    }
}

fn color(value: &str) -> Hsla {
    let hex = value.trim_start_matches('#');
    let value =
        u32::from_str_radix(hex, 16).expect("theme colors are validated before application");
    // RGBA uses a trailing alpha byte; six-digit themes retain full opacity.
    if hex.len() == 8 {
        rgba(value).into()
    } else {
        rgb(value).into()
    }
}

/// Publishes the shared typography size to GPUI Kit's global theme.
pub fn sync_font_sizes(cx: &mut App) {
    let typography = cx
        .has_global::<RuntimeStyles>()
        .then(|| cx.global::<RuntimeStyles>().typography.clone())
        .unwrap_or_default();
    apply_font_sizes(cx, &typography);
}

fn apply_font_sizes(cx: &mut App, typography: &ThemeTypography) {
    let base = crate::typography::font_size(cx);
    let editor = crate::typography::editor_font_size(cx);

    let theme = Theme::global_mut(cx);
    theme.font_size = typography.ui.size_px.map(px).unwrap_or(base);
    theme.mono_font_size = typography.mono.size_px.map(px).unwrap_or(editor);
}

/// Expose validated theme tokens through the generic runtime-plugin environment.
pub fn plugin_colors(cx: &App) -> BTreeMap<String, u32> {
    cx.global::<RuntimeStyles>().plugin_colors.clone()
}

pub fn plugin_text_styles(cx: &App) -> BTreeMap<String, PluginTextStyle> {
    cx.global::<RuntimeStyles>().plugin_text_styles.clone()
}

/// Prefer an enabled theme package while retaining a usable palette before installation.
pub fn active_theme(dark: bool) -> ThemeDefinition {
    crate::extensions::contributions::theme(dark).unwrap_or_else(|| builtin_theme(dark).clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Rgba, TestAppContext, gpui};

    /// RGB keeps old themes opaque; RGBA preserves transparent and partial fills.
    #[test]
    fn theme_color_alpha_is_not_discarded() {
        for (source, alpha) in [
            ("#123456", 1.),
            ("#12345680", 128. / 255.),
            ("#12345600", 0.),
        ] {
            let rgba = Rgba::from(color(source));
            assert!((rgba.r - 0x12 as f32 / 255.).abs() < 0.00001);
            assert!((rgba.a - alpha).abs() < 0.00001);
        }
    }

    /// Theme updates must keep alpha in both GPUI colors and the Root's semantic tokens.
    #[gpui::test]
    fn translucent_theme_reaches_component_and_base_colors(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::typography::init(cx);
            let mut theme = builtin_theme(false).clone();
            theme.colors.background = "#ffffff80".into();
            theme
                .components
                .get_mut(&ThemeComponent::Editor)
                .unwrap()
                .base
                .background = Some("#ffffff40".into());
            theme
                .components
                .get_mut(&ThemeComponent::ExplorerMenu)
                .unwrap()
                .base
                .background = Some("#f7f8fa80".into());
            apply_theme(&theme, cx);
            let expected = 128. / 255.;
            assert!((Theme::global(cx).background.a - expected).abs() < 0.00001);
            assert!((Theme::global(cx).tokens.background.color.a - expected).abs() < 0.00001);
            assert!(
                (gpui_base::Theme::global(cx).tokens.colors.background.a - expected).abs()
                    < 0.00001
            );
            assert!(
                (component_styles(cx, ThemeComponent::ExplorerMenu)
                    .base
                    .background
                    .unwrap()
                    .a
                    - expected)
                    .abs()
                    < 0.00001
            );
            assert!(
                (Theme::global(cx)
                    .highlight_theme
                    .style
                    .editor_background
                    .unwrap()
                    .a
                    - 64. / 255.)
                    .abs()
                    < 0.00001
            );
            // Returning to an old RGB theme must remove the previous alpha settings.
            apply_theme(builtin_theme(false), cx);
            assert_eq!(Theme::global(cx).tokens.background.color.a, 1.);
            assert_eq!(
                Theme::global(cx)
                    .highlight_theme
                    .style
                    .editor_background
                    .unwrap()
                    .a,
                1.
            );
        });
    }

    /// The embedded palettes remain available even when no theme plugin is installed.
    #[test]
    fn builtin_themes_include_default_light_and_dark_palettes() {
        let light = builtin_theme(false);
        let dark = builtin_theme(true);
        assert_eq!(light.id, "builtin-light");
        assert_eq!(light.mode, FileThemeMode::Light);
        assert_eq!(light.colors.background, "#ffffff");
        assert_eq!(dark.mode, FileThemeMode::Dark);
        assert_eq!(dark.colors.background, "#1e1f22");
        for theme in [light, dark] {
            let ansi = theme
                .plugin_colors()
                .keys()
                .filter(|key| key.starts_with("terminal.ansi."))
                .count();
            assert_eq!(
                ansi, 0,
                "{} must leave terminal domain defaults to its package",
                theme.id
            );
        }
    }
}
