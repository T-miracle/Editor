//! Loads theme definitions and applies their colors to GPUI.

use std::{collections::BTreeMap, sync::LazyLock, time::Duration};

use gpui_kit::{
    App, Global, Hsla,
    component::{Theme, ThemeMode},
    px, rgb,
};
use plugin_schema::{
    ComponentStyles, StyleProperties, ThemeComponent, ThemeDefinition, ThemeFile,
    ThemeMode as FileThemeMode,
};

static BUILTIN_THEME_FILE: LazyLock<ThemeFile> = LazyLock::new(|| {
    ThemeFile::parse(include_str!(
        "../../../../plugins/default-light-theme/theme.json"
    ))
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
    let scrollbar = theme.components.get(&ThemeComponent::Scrollbar);
    let scrollbar_thumb = scrollbar
        .and_then(|styles| styles.base.background.as_deref())
        .map(color)
        .unwrap_or(accent);
    let scrollbar_thumb_hover = scrollbar
        .and_then(|styles| styles.hover.as_ref())
        .and_then(|styles| styles.background.as_deref())
        .map(color)
        .unwrap_or(accent);
    let scrollbar_thumb_active = scrollbar
        .and_then(|styles| styles.active.as_ref())
        .and_then(|styles| styles.background.as_deref())
        .map(color)
        .unwrap_or(accent);
    // Theme files use opaque RGB values; apply transparency to scrollbar thumbs here.
    let scrollbar_thumb = scrollbar_thumb.opacity(0.55);
    let scrollbar_thumb_hover = scrollbar_thumb_hover.opacity(0.7);
    let scrollbar_thumb_active = scrollbar_thumb_active.opacity(0.8);

    {
        let theme = Theme::global_mut(cx);
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
    }
    sync_font_sizes(cx);
    Theme::sync_base(cx);

    let base_theme = gpui_base::Theme::global_mut(cx);
    let scrollbar = base_theme.scrollbar.clone();
    let motion = scrollbar.motion().with_idle(Duration::from_secs(2));
    // Keep explorer and editor scrollbars visible for two seconds after activity.
    base_theme.scrollbar = scrollbar.with_motion(motion).with_styles(
        gpui_base::ScrollbarStyles::default()
            // Use one eight-pixel width in every state so hovering cannot resize the bar.
            .track(|style| style.width(px(8.)))
            .track_hover(|style| style.width(px(8.)))
            .track_active(|style| style.width(px(8.)))
            .thumb(|style| {
                style
                    .bg(scrollbar_thumb)
                    .width(px(8.))
                    .inset(px(0.))
                    .radius(px(4.))
            })
            .thumb_hover(|style| {
                style
                    .bg(scrollbar_thumb_hover)
                    .width(px(8.))
                    .inset(px(0.))
                    .radius(px(4.))
            })
            .thumb_active(|style| {
                style
                    .bg(scrollbar_thumb_active)
                    .width(px(8.))
                    .inset(px(0.))
                    .radius(px(4.))
            }),
    );

    let components = theme
        .components
        .iter()
        .map(|(component, styles)| (*component, resolve_component_styles(styles)))
        .collect();
    if cx.has_global::<RuntimeStyles>() {
        cx.global_mut::<RuntimeStyles>().components = components;
    } else {
        cx.set_global(RuntimeStyles { components });
    }
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
    let value = u32::from_str_radix(value.trim_start_matches('#'), 16)
        .expect("theme colors are validated before application");
    rgb(value).into()
}

/// Publishes the shared typography size to GPUI Kit's global theme.
pub fn sync_font_sizes(cx: &mut App) {
    let base = crate::typography::font_size(cx);
    let editor = crate::typography::editor_font_size(cx);

    let theme = Theme::global_mut(cx);
    theme.font_size = base;
    theme.mono_font_size = editor;
    // Upstream popovers inherit the shared theme typography.
}

/// Prefer an enabled theme package while retaining a usable palette before installation.
pub fn active_theme(dark: bool) -> ThemeDefinition {
    crate::extensions::contributions::theme(dark).unwrap_or_else(|| builtin_theme(dark).clone())
}
