use gpui_kit::{
    App, Hsla,
    component::{Theme, ThemeMode},
    px, rgb,
};

/// Installs a compact JetBrains 2023-inspired palette over GPUI Kit's theme.
pub fn apply_jetbrains_theme(dark: bool, cx: &mut App) {
    Theme::change(
        if dark {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        },
        None,
        cx,
    );

    let palette = if dark {
        Palette::dark()
    } else {
        Palette::light()
    };
    {
        let theme = Theme::global_mut(cx);
        theme.background = palette.background;
        theme.foreground = palette.foreground;
        theme.border = palette.border;
        theme.sidebar = palette.surface;
        theme.sidebar_foreground = palette.foreground;
        theme.sidebar_border = palette.border;
        theme.sidebar_accent = palette.hover;
        theme.sidebar_accent_foreground = palette.foreground;
        theme.tab_bar = palette.surface;
        theme.tab = palette.surface;
        theme.tab_foreground = palette.muted_foreground;
        theme.tab_active = palette.background;
        theme.tab_active_foreground = palette.foreground;
        theme.muted = palette.surface;
        theme.muted_foreground = palette.muted_foreground;
        theme.input = palette.border;
        theme.colors.list = palette.surface;
        theme.list_hover = palette.hover;
        theme.list_active = palette.selection;
        theme.list_active_border = palette.accent;
        theme.selection = palette.selection;
        theme.accent = palette.hover;
        theme.accent_foreground = palette.foreground;
        theme.primary = palette.accent;
        theme.primary_hover = palette.accent_hover;
        theme.primary_active = palette.accent_active;
        theme.primary_foreground = color(0xffffff);
        theme.button = palette.surface;
        theme.button_hover = palette.hover;
        theme.button_active = palette.border;
        theme.button_foreground = palette.foreground;
        theme.button_primary = palette.accent;
        theme.button_primary_hover = palette.accent_hover;
        theme.button_primary_active = palette.accent_active;
        theme.button_primary_foreground = color(0xffffff);
        theme.popover = palette.surface;
        theme.popover_foreground = palette.foreground;
        theme.ring = palette.accent;
        theme.drag_border = palette.accent;
        theme.caret = palette.accent;
        theme.radius = px(6.);
        theme.radius_lg = px(8.);
    }
    sync_font_sizes(cx);
    Theme::sync_base(cx);

    // Keep the explorer's scrollbar slim while retaining a 6px hit target so
    // it remains easy to grab. The accent matches the tabs scrollbar thumb.
    let base_theme = gpui_base::Theme::global_mut(cx);
    let scrollbar = base_theme.scrollbar.clone();
    base_theme.scrollbar = scrollbar.with_styles(
        gpui_base::ScrollbarStyles::default()
            .track(|style| style.width(px(6.)))
            .track_hover(|style| style.width(px(6.)))
            .track_active(|style| style.width(px(6.)))
            .thumb(|style| {
                style
                    .bg(palette.accent)
                    .width(px(2.))
                    .inset(px(0.))
                    .radius(px(2.))
            })
            .thumb_hover(|style| {
                style
                    .bg(palette.accent)
                    .width(px(6.))
                    .inset(px(0.))
                    .radius(px(3.))
            })
            .thumb_active(|style| {
                style
                    .bg(palette.accent)
                    .width(px(6.))
                    .inset(px(0.))
                    .radius(px(3.))
            }),
    );
}

/// Publishes [`crate::typography`]'s sizes onto the theme.
///
/// `Theme::font_size` is the interface's base size and the value `Root` pushes
/// onto the window as its `rem` size; `mono_font_size` is what anything else
/// monospaced draws at. Both are derived from the one font size, which is what
/// keeps the explorer's rows and the editor's text in step.
pub fn sync_font_sizes(cx: &mut App) {
    let base = crate::typography::font_size(cx);
    let editor = crate::typography::editor_font_size(cx);

    let theme = Theme::global_mut(cx);
    theme.font_size = base;
    theme.mono_font_size = editor;
}

#[derive(Clone, Copy)]
struct Palette {
    background: Hsla,
    surface: Hsla,
    hover: Hsla,
    border: Hsla,
    foreground: Hsla,
    muted_foreground: Hsla,
    selection: Hsla,
    accent: Hsla,
    accent_hover: Hsla,
    accent_active: Hsla,
}

impl Palette {
    fn dark() -> Self {
        Self {
            background: color(0x1e1f22),
            surface: color(0x2b2d30),
            hover: color(0x393b40),
            border: color(0x43454a),
            foreground: color(0xdfe1e5),
            muted_foreground: color(0x9da0a8),
            selection: color(0x2e436e),
            accent: color(0x3574f0),
            accent_hover: color(0x4682fa),
            accent_active: color(0x2f65ca),
        }
    }

    fn light() -> Self {
        Self {
            background: color(0xffffff),
            surface: color(0xf7f8fa),
            hover: color(0xebecf0),
            border: color(0xd3d5db),
            foreground: color(0x1f2329),
            muted_foreground: color(0x6c707e),
            selection: color(0xd4e2ff),
            accent: color(0x3574f0),
            accent_hover: color(0x2f65ca),
            accent_active: color(0x2554a8),
        }
    }
}

fn color(value: u32) -> Hsla {
    rgb(value).into()
}
