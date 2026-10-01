//! Compose the SVG stage in explicit back-to-front order over the ordinary preview panel.

use super::*;

impl State {
    /// The checkerboard is separate paint underneath the SVG, never injected into the document.
    pub(super) fn scene(&self) -> Scene {
        let environment = &self.environment;
        let font = environment
            .ui_font
            .family
            .clone()
            .unwrap_or_else(|| "Segoe UI".into());
        let font_size = environment.ui_font.size_px.unwrap_or(14.).clamp(1., 128.);
        let mut scene = Scene {
            panel: "preview".into(),
            font,
            font_size,
            ..Default::default()
        };
        scene.paint.push(Paint::Fill {
            rect: Rect {
                x: 0.,
                y: 0.,
                w: self.width,
                h: self.height,
            },
            color: environment.background,
            extend_to_bottom: true,
        });
        self.toolbar(&mut scene.paint, font_size);
        scene.paint.push(Paint::Fill {
            rect: Rect {
                x: 0.,
                y: self.toolbar_height() - 1.,
                w: self.width,
                h: 1.,
            },
            color: environment.border,
            extend_to_bottom: false,
        });
        if let Some(image) = self.image_rect() {
            checkerboard(&mut scene.paint, image, self.viewport());
            // Drawing last preserves all SVG colors and reveals the board through alpha holes.
            scene.paint.push(Paint::Svg {
                rect: image,
                clip: self.viewport(),
                source: self.source.clone(),
            });
        } else {
            let title = if self.error.is_some() {
                "SVG 无法预览"
            } else {
                "打开 SVG 文件以预览"
            };
            scene.paint.push(Paint::Text {
                x: 16.,
                y: self.toolbar_height() + 24.,
                text: title.into(),
                color: environment.muted_foreground,
                size: 14.,
                bold: false,
                font: None,
            });
            if let Some(error) = &self.error {
                scene.paint.push(Paint::Text {
                    x: 16.,
                    y: self.toolbar_height() + 50.,
                    text: error.chars().take(180).collect(),
                    color: environment.muted_foreground,
                    size: 12.,
                    bold: false,
                    font: None,
                });
            }
        }
        scene
    }

    /// Paint themed SVG controls and percentage text using the host's default UI typography.
    fn toolbar(&self, paint: &mut Vec<Paint>, font_size: f32) {
        let color = if self.intrinsic.is_some() {
            self.environment.foreground
        } else {
            self.environment.muted_foreground
        };
        let clip = Rect {
            x: 0.,
            y: 0.,
            w: self.width,
            h: self.toolbar_height(),
        };
        for (index, (_, source)) in TOOLBAR_ICONS.iter().enumerate() {
            let target = self.toolbar_button_rect(index);
            if self.hovered_button == Some(index) && self.intrinsic.is_some() {
                paint.push(Paint::Fill {
                    rect: target,
                    color: self.environment.muted,
                    extend_to_bottom: false,
                });
            }
            let icon_size = 20_f32.min(target.w);
            if icon_size > 0. {
                paint.push(Paint::Svg {
                    rect: Rect {
                        x: target.x + (target.w - icon_size) / 2.,
                        y: 6.,
                        w: icon_size,
                        h: 20.,
                    },
                    clip,
                    source: source.replace("currentColor", &format!("#{color:06x}")),
                });
            }
        }
        let percent = self.scale * 100.;
        let label = if self.intrinsic.is_none() {
            "—".into()
        } else if percent < 1. {
            format!("{percent:.2}%")
        } else {
            format!("{percent:.0}%")
        };
        // Leave room for proportional glyphs without overriding the editor's default font family.
        let row_top = if self.width < 220. { 32. } else { 0. };
        let row_height = self.toolbar_height() - row_top;
        paint.push(Paint::Text {
            x: (self.width - 12. - label.chars().count() as f32 * font_size * 0.65).max(0.),
            y: row_top + ((row_height - (font_size * 1.45).ceil()) / 2.).max(0.),
            text: label,
            color: self.environment.foreground,
            size: font_size,
            bold: self.environment.ui_font.bold.unwrap_or(false),
            // None inherits Scene.font, which is populated from the editor's UI font family.
            font: None,
        });
    }
}

/// Tile only the visible board so even extreme zoom stays within the drawing-operation quota.
fn checkerboard(paint: &mut Vec<Paint>, image: Rect, viewport: Rect) {
    let left = image.x.max(viewport.x);
    let top = image.y.max(viewport.y);
    let right = (image.x + image.w).min(viewport.x + viewport.w);
    let bottom = (image.y + image.h).min(viewport.y + viewport.h);
    if right <= left || bottom <= top {
        return;
    }
    paint.push(Paint::Fill {
        rect: Rect {
            x: left,
            y: top,
            w: right - left,
            h: bottom - top,
        },
        color: 0xffffff,
        extend_to_bottom: false,
    });
    let tile = 8_f32.max(((right - left) * (bottom - top) / 16_000.).sqrt());
    let first_column = ((left - image.x) / tile).floor() as i32;
    let first_row = ((top - image.y) / tile).floor() as i32;
    let last_column = ((right - image.x) / tile).ceil() as i32;
    let last_row = ((bottom - image.y) / tile).ceil() as i32;
    for row in first_row..last_row {
        for column in first_column..last_column {
            if (row + column) % 2 == 0 {
                continue;
            }
            let x = (image.x + column as f32 * tile).max(left);
            let y = (image.y + row as f32 * tile).max(top);
            let w = (image.x + (column + 1) as f32 * tile).min(right) - x;
            let h = (image.y + (row + 1) as f32 * tile).min(bottom) - y;
            if w > 0. && h > 0. {
                paint.push(Paint::Fill {
                    rect: Rect { x, y, w, h },
                    color: 0xbfbfbf,
                    extend_to_bottom: false,
                });
            }
        }
    }
}
