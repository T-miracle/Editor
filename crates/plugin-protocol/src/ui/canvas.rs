//! Optional drawing and input contracts, independent of terminal state and native form geometry.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Canvas {
    pub paint: Vec<crate::Paint>,
    /// Ordinary image surfaces do not claim text focus or IME; interactive canvases explicitly opt in.
    #[serde(default)]
    pub focusable: bool,
    /// Requires ui.grid negotiation; without it Resize contains no character-cell metrics.
    #[serde(default)]
    pub grid: bool,
    /// Optional IME anchor in canvas-local coordinates. It does not imply an editable document.
    #[serde(default)]
    pub caret: Option<crate::Rect>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GridMetrics {
    pub cell_width: f32,
    pub cell_height: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointerPhase {
    Down,
    Move,
    Up,
}

/// Events target their enclosing UiEvent node and revision, never a global plugin input stream.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CanvasEvent {
    Resize {
        width: f32,
        height: f32,
        grid: Option<GridMetrics>,
    },
    Focus {
        focused: bool,
    },
    Key {
        key: String,
        ctrl: bool,
        alt: bool,
        shift: bool,
    },
    Text {
        text: String,
    },
    Pointer {
        phase: PointerPhase,
        x: f32,
        y: f32,
        button: u8,
        clicks: u8,
        shift: bool,
    },
    Wheel {
        x: f32,
        y: f32,
        delta_x: f32,
        delta_y: f32,
        shift: bool,
    },
}

impl Canvas {
    /// Validate geometry and bound expensive vectors before allocating host rendering resources.
    pub fn validate(&self) -> Result<(), String> {
        let coordinate = |value: f32| value.is_finite() && value.abs() <= 1_000_000.;
        let rect = |rect: &crate::Rect| {
            coordinate(rect.x)
                && coordinate(rect.y)
                && coordinate(rect.w)
                && coordinate(rect.h)
                && rect.w >= 0.
                && rect.h >= 0.
        };
        if self.paint.len() > 32_000 || self.caret.as_ref().is_some_and(|value| !rect(value)) {
            return Err("Invalid canvas geometry or drawing quota".into());
        }
        let mut vectors = 0;
        let mut bytes = 0;
        for paint in &self.paint {
            let valid = match paint {
                crate::Paint::Fill { rect: value, .. } => rect(value),
                crate::Paint::Text {
                    x,
                    y,
                    text,
                    size,
                    font,
                    ..
                } => {
                    bytes += text.len();
                    coordinate(*x)
                        && coordinate(*y)
                        && (1. ..=128.).contains(size)
                        && text.len() <= 65536
                        && font
                            .as_ref()
                            .is_none_or(|font| !font.is_empty() && font.len() <= 256)
                }
                crate::Paint::Svg {
                    rect: value,
                    clip,
                    source,
                } => {
                    vectors += 1;
                    bytes += source.len();
                    rect(value) && rect(clip) && !source.is_empty() && source.len() <= 1024 * 1024
                }
            };
            if !valid || vectors > 16 || bytes > 2 * 1024 * 1024 {
                return Err("Invalid canvas paint or content quota".into());
            }
        }
        Ok(())
    }
}
