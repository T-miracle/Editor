//! The one font size every piece of text in the editor is derived from.
//!
//! [`Typography::font_size`] is the sole input. It lands in two places that
//! between them cover all text:
//!
//! - `Theme::font_size`, which `Root` pushes onto the window as the `rem` size
//!   every frame. Every `rem`-classed size resolves against it, so this is what
//!   scales the interface: the explorer's rows, the status bar, buttons, and the
//!   `rem`-sized icons.
//! - the code editor's own size, read from [`Typography::editor_font_size`] at
//!   render time.
//!
//! Both come from the same field, so the explorer and the editor cannot drift
//! apart. Editing this value is how "make everything bigger" is expressed; there
//! is deliberately no second size to keep in sync.

use gpui_kit::{App, Global, Pixels, px};

/// Base font size, in pixels, that the editor and the whole interface open at.
const DEFAULT_FONT_SIZE: f32 = 14.;
/// Smallest base font size the user can dial down to.
const MIN_FONT_SIZE: f32 = 10.;
/// Largest base font size the user can dial up to.
const MAX_FONT_SIZE: f32 = 24.;
/// How far one step of the font size controls moves the base font size.
const FONT_SIZE_STEP: f32 = 1.;

/// The user's text sizing, held as a global so layout code can read it without
/// threading it through every view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Typography {
    font_size: f32,
}

impl Default for Typography {
    fn default() -> Self {
        Self {
            font_size: DEFAULT_FONT_SIZE,
        }
    }
}

impl Global for Typography {}

impl Typography {
    /// The base font size, in pixels. Every text size in the app starts here.
    pub fn font_size(&self) -> Pixels {
        px(self.font_size)
    }

    /// The size the code editor draws its text at.
    ///
    /// No separate knob: the editor opens at the interface's size and follows it.
    /// A future feature that sizes the editor independently belongs here as a
    /// documented offset from [`Self::font_size`], not as a second field.
    pub fn editor_font_size(&self) -> Pixels {
        self.font_size()
    }

    /// Moves the base font size by `steps`, clamped to the supported range.
    ///
    /// Steps land on whole pixels, so repeated presses cannot accumulate
    /// fractional drift in the base that everything else multiplies.
    pub fn step_by(&mut self, steps: i32) -> Pixels {
        self.set_font_size(self.font_size + FONT_SIZE_STEP * steps as f32)
    }

    /// Sets the base font size, clamped to the supported range and rounded to
    /// whole pixels so the value everything multiplies stays exact.
    pub fn set_font_size(&mut self, font_size: f32) -> Pixels {
        self.font_size = font_size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE).round();

        self.font_size()
    }
}

/// The base font size the app opens at.
///
/// Only tests need this: production code reads the live value from the global,
/// and layout that depends on the opening size would be wrong the moment the
/// user changed it.
#[cfg(test)]
pub fn base_font_size() -> Pixels {
    Typography::default().font_size()
}

/// Installs the typography global, seeded with [`Typography::default`].
pub fn init(cx: &mut App) {
    cx.set_global(Typography::default());
}

/// Reads the current base font size out of `app`.
pub fn font_size(app: &App) -> Pixels {
    app.global::<Typography>().font_size()
}

/// Reads the size the code editor draws its text at.
pub fn editor_font_size(app: &App) -> Pixels {
    app.global::<Typography>().editor_font_size()
}

/// Moves the base font size and returns the value it settled on.
pub fn step_by(app: &mut App, steps: i32) -> Pixels {
    app.global_mut::<Typography>().step_by(steps)
}

/// Sets the base font size outright, clamped to the supported range.
///
/// The stepping controls are the user-facing way to move the size; this is for
/// callers that already know the size they want, tests included.
#[cfg(test)]
pub fn set_font_size(app: &mut App, font_size: f32) {
    app.global_mut::<Typography>().set_font_size(font_size);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_at_the_configured_default() {
        let typography = Typography::default();

        assert_eq!(typography.font_size(), px(14.));
        assert_eq!(base_font_size(), typography.font_size());
    }

    /// The explorer's rows and the editor's text must not be able to disagree:
    /// they are the same field.
    #[test]
    fn the_editor_and_the_interface_share_one_size() {
        let mut typography = Typography::default();

        for steps in [-3, -1, 0, 1, 5] {
            typography.step_by(steps);
            assert_eq!(
                typography.editor_font_size(),
                typography.font_size(),
                "the editor drifted from the interface after {steps} step(s)"
            );
        }
    }

    #[test]
    fn stepping_moves_whole_pixels_without_drifting() {
        let mut typography = Typography::default();

        assert_eq!(typography.step_by(1), px(15.));
        assert_eq!(typography.step_by(1), px(16.));
        assert_eq!(typography.step_by(-2), px(14.));
        assert_eq!(typography.step_by(0), px(14.));
    }

    #[test]
    fn stepping_stops_at_the_supported_range() {
        let mut typography = Typography::default();

        typography.step_by(-100);
        assert_eq!(typography.font_size(), px(MIN_FONT_SIZE));

        typography.step_by(100);
        assert_eq!(typography.font_size(), px(MAX_FONT_SIZE));
    }
}
