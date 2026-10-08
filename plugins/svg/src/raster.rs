//! Raster sizing and wheel policy live in the guest; the host only reports geometry and paints.
use super::*;

#[derive(Default)]
pub(super) struct Raster {
    /// Absolute intrinsic scale is transient intent for the current exact file context.
    pub scale: Option<f32>,
    content: Option<ui::ContentSize>,
    area: Option<ui::ContentSize>,
    manual: bool,
}
impl Raster {
    /// Expose center-anchored projection using the public capability, without transferring policy.
    pub fn viewport(&self) -> ui::VisualViewport {
        ui::VisualViewport {
            content: None,
            transform: self.scale.map(|scale| ui::ContentTransform {
                scale,
                ..Default::default()
            }),
        }
    }
    /// Adopt fresh intrinsic dimensions, follow automatic containment, or preserve explicit user zoom.
    pub fn input(&mut self, input: ui::ViewportInput, line: f32) -> bool {
        if self.content != Some(input.content) {
            self.content = Some(input.content);
            self.scale = None;
            self.manual = false;
        }
        match input.event {
            ui::CanvasEvent::Resize { width, height, .. } if width > 0. && height > 0. => {
                self.area = Some(ui::ContentSize { width, height });
                if !self.manual {
                    self.scale = Some(self.contain(input.content));
                }
                true
            }
            ui::CanvasEvent::Wheel { delta_y, .. } if delta_y.is_finite() && delta_y != 0. => {
                let scale = self.scale.unwrap_or_else(|| self.contain(input.content));
                let step = (delta_y / line.max(1.)).clamp(-100., 100.);
                // Raster and SVG share the product's wheel direction and step; extents remain protocol-bounded.
                let ceiling =
                    MAX_SCALE.min(MAX_EXTENT / input.content.width.max(input.content.height));
                self.scale =
                    Some((scale * 1.12_f32.powf(step)).clamp(MIN_SCALE.min(ceiling), ceiling));
                self.manual = true;
                true
            }
            _ => false,
        }
    }
    /// Original containment is the raster default; SVG's 240px minimum is its own separate policy.
    fn contain(&self, content: ui::ContentSize) -> f32 {
        let area = self.area.unwrap_or(ui::ContentSize {
            width: 400.,
            height: 300.,
        });
        (area.width / content.width)
            .min(area.height / content.height)
            .min(1.)
    }
}
