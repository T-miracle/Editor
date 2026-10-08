//! Shared visual geometry: guests choose zoom/pan policy; native hosts project and clip pixels.
use super::*;

/// Logical, unscaled content dimensions. Images report decoded intrinsic size; canvases declare it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentSize {
    /// Intrinsic width in logical pixels, finite and positive, at most one million.
    pub width: f32,
    /// Intrinsic height under the same quota.
    pub height: f32,
}

/// Uniform scale and translation in viewport pixels, around a normalized content/viewport anchor.
/// The default keeps the content center at the viewport center. No wheel policy is implicit.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentTransform {
    /// Absolute uniform scale relative to the unscaled content, finite and positive.
    pub scale: f32,
    /// Horizontal translation in viewport logical pixels.
    pub x: f32,
    /// Vertical translation in viewport logical pixels.
    pub y: f32,
    /// Shared content/viewport horizontal anchor fraction in [0,1].
    pub anchor_x: f32,
    /// Shared content/viewport vertical anchor fraction in [0,1].
    pub anchor_y: f32,
}
impl Default for ContentTransform {
    fn default() -> Self {
        Self {
            scale: 1.,
            x: 0.,
            y: 0.,
            anchor_x: 0.5,
            anchor_y: 0.5,
        }
    }
}
impl ContentTransform {
    /// Project a content-space rectangle into a local viewport of `area` using intrinsic `content`.
    /// Returns logical pixels; callers validate the declaration and clip to the viewport bounds.
    pub fn project(
        &self,
        rect: crate::Rect,
        content: ContentSize,
        area: ContentSize,
    ) -> crate::Rect {
        crate::Rect {
            x: (rect.x - content.width * self.anchor_x) * self.scale
                + area.width * self.anchor_x
                + self.x,
            y: (rect.y - content.height * self.anchor_y) * self.scale
                + area.height * self.anchor_y
                + self.y,
            w: rect.w * self.scale,
            h: rect.h * self.scale,
        }
    }
}

/// Opt-in visual input and projection on FileImage, Image and Canvas, requiring `ui.viewport`.
/// Images derive `content` from decoded pixels and must leave it absent; Canvas requires it.
/// With no transform an image retains its sizing policy until its first measurement reaches the guest.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VisualViewport {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<ContentSize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform: Option<ContentTransform>,
}

/// Native visual input carries measured unscaled dimensions; pointer coordinates stay viewport-local.
/// It carries no file bytes or extra authority. Normal node, modal and revision validation applies.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewportInput {
    pub content: ContentSize,
    pub event: CanvasEvent,
}

impl ContentSize {
    /// Reject non-finite, empty and excessive dimensions before any layout or guest callback.
    pub(super) fn valid(&self) -> bool {
        [self.width, self.height]
            .into_iter()
            .all(|v| v.is_finite() && v > 0. && v <= 1_000_000.)
    }
}
impl VisualViewport {
    /// Validate capability geometry independently of native widget types and plugin identity.
    pub(super) fn validate(&self, kind: &Kind) -> Result<(), String> {
        match kind {
            Kind::Canvas(canvas) if !canvas.grid && canvas.scroll.is_none() && self.content.is_some_and(|size| size.valid()) => {}
            Kind::Image { .. } | Kind::FileImage { .. } if self.content.is_none() => {}
            _ => return Err("Visual viewport requires an image or a nongrid canvas with content dimensions and no scroll range".into()),
        }
        if let Some(t) = self.transform {
            if !t.scale.is_finite()
                || t.scale <= 0.
                || t.scale > 1_000_000.
                || ![t.x, t.y]
                    .into_iter()
                    .all(|v| v.is_finite() && v.abs() <= 1_000_000.)
                || ![t.anchor_x, t.anchor_y]
                    .into_iter()
                    .all(|v| v.is_finite() && (0. ..=1.).contains(&v))
            {
                return Err("Invalid content transform".into());
            }
            if self.content.is_some_and(|size| {
                size.width * t.scale > 1_000_000. || size.height * t.scale > 1_000_000.
            }) {
                return Err("Transformed content exceeds the drawing extent quota".into());
            }
        }
        // Bound projected paint too: small declared content cannot hide enormous transformed operations.
        if let Kind::Canvas(canvas) = kind {
            let t = self.transform.unwrap_or_default();
            let content = self.content.unwrap();
            for paint in &canvas.paint {
                if let crate::Paint::Text { size, .. } = paint {
                    if !(1. ..=128.).contains(&(*size * t.scale)) {
                        return Err("Transformed text exceeds the font-size quota".into());
                    }
                }
                let rect = match paint {
                    crate::Paint::Fill { rect, .. } | crate::Paint::Svg { rect, .. } => *rect,
                    crate::Paint::Text { x, y, size, .. } => crate::Rect {
                        x: *x,
                        y: *y,
                        w: 0.,
                        h: *size,
                    },
                };
                let rect = t.project(
                    rect,
                    content,
                    ContentSize {
                        width: 0.,
                        height: 0.,
                    },
                );
                if [rect.x, rect.y, rect.w, rect.h]
                    .into_iter()
                    .any(|v| !v.is_finite() || v.abs() > 1_000_000.)
                {
                    return Err("Transformed drawing exceeds the extent quota".into());
                }
            }
        }
        Ok(())
    }
}
