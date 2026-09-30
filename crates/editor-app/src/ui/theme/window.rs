//! Apply native materials through the shared Base Root without repainting its background.

use gpui_base::{Root, RootPlugin};
use gpui_kit::{
    App, Context, Div, IntoElement, Render, Stateful, Styled as _, Window,
    WindowBackgroundAppearance, div, transparent_black,
};
use plugin_schema::ThemeWindowBackground;

use super::RuntimeStyles;

/// Creation options use the same material as already mounted windows.
pub fn window_background(cx: &App) -> WindowBackgroundAppearance {
    match cx.global::<RuntimeStyles>().window.background_appearance {
        ThemeWindowBackground::Opaque => WindowBackgroundAppearance::Opaque,
        ThemeWindowBackground::Transparent => WindowBackgroundAppearance::Transparent,
        ThemeWindowBackground::Blurred => WindowBackgroundAppearance::Blurred,
    }
}

/// Registration is idempotent and applies to the main window and native dialogs.
pub(super) fn register(cx: &mut App) {
    Root::register_plugin(cx, |_, _| WindowMaterial { applied: None });
}

/// Each window tracks its applied material to avoid OS calls on every frame.
struct WindowMaterial {
    applied: Option<WindowBackgroundAppearance>,
}

impl RootPlugin for WindowMaterial {
    fn prepare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let appearance = window_background(cx);
        if self.applied != Some(appearance) {
            window.set_background_appearance(appearance);
            self.applied = Some(appearance);
        }
    }

    fn style(&self, surface: &mut Stateful<Div>, _: &mut Window, _: &mut App) {
        // The app shell owns its fill. A second root fill would accumulate alpha
        // or hide the OS backdrop beneath an opaque component-library default.
        surface.style().background = Some(transparent_black().into());
    }
}

impl Render for WindowMaterial {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // Material configuration contributes no visible overlay or hit target.
        div()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{apply_theme, builtin_theme};
    use gpui_kit::{AppContext as _, TestAppContext, gpui, px, size};

    struct Content;
    impl Render for Content {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    /// Existing windows follow material changes, and windows opened later inherit them.
    #[gpui::test]
    fn theme_switch_updates_existing_and_new_window_materials(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::typography::init(cx);
            apply_theme(builtin_theme(false), cx);
        });
        let (root, visual) =
            cx.add_window_view(|window, cx| Root::new(cx.new(|_| Content), window, cx));
        visual.simulate_resize(size(px(300.), px(200.)));
        for (mode, expected) in [
            (
                ThemeWindowBackground::Blurred,
                WindowBackgroundAppearance::Blurred,
            ),
            (
                ThemeWindowBackground::Transparent,
                WindowBackgroundAppearance::Transparent,
            ),
            (
                ThemeWindowBackground::Opaque,
                WindowBackgroundAppearance::Opaque,
            ),
        ] {
            // Run inside a window update, as the settings theme toggle does.
            visual.update(|_, cx| {
                let mut theme = builtin_theme(false).clone();
                theme.window.background_appearance = mode;
                apply_theme(&theme, cx);
            });
            visual.update(|window, cx| window.draw(cx).clear(cx));
            root.read_with(visual, |root, cx| {
                assert_eq!(
                    root.plugin::<WindowMaterial>().unwrap().read(cx).applied,
                    Some(expected)
                );
            });
            visual.update(|_, cx| assert_eq!(window_background(cx), expected));
        }
        cx.update(|cx| {
            let mut theme = builtin_theme(false).clone();
            theme.window.background_appearance = ThemeWindowBackground::Blurred;
            apply_theme(&theme, cx);
        });
        let (new_root, new_visual) =
            cx.add_window_view(|window, cx| Root::new(cx.new(|_| Content), window, cx));
        new_visual.simulate_resize(size(px(300.), px(200.)));
        new_visual.update(|window, cx| window.draw(cx).clear(cx));
        new_root.read_with(new_visual, |root, cx| {
            assert_eq!(
                root.plugin::<WindowMaterial>().unwrap().read(cx).applied,
                Some(WindowBackgroundAppearance::Blurred)
            );
        });
    }
}
