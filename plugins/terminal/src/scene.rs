//! Translate emulator cells into generic native drawing operations.
use super::*;
use crate::emulator::Color;

/// Alacritty allocates history only when real rows leave the screen.
pub(super) fn visible_history(tab: &Tab) -> usize {
    tab.term.history()
}
impl Terminal {
    /// The host receives only drawing primitives and generic editable widget descriptions.
    pub(super) fn scene(&self) -> Scene {
        let bg = self.color(257);
        let content_style = self.text_style("content", true);
        let error_style = self.text_style("error", false);
        let content_size = content_style.size_px.unwrap();
        let right = self.tab_left();
        let mut scene = Scene {
            panel: "terminal".into(),
            font: content_style.family.clone().unwrap(),
            font_size: content_size,
            controls: Some(self.canvas_controls()),
            ..Scene::default()
        };
        fill_to_bottom(
            &mut scene,
            Rect {
                x: 0.,
                y: 0.,
                w: self.width,
                h: self.height,
            },
            bg,
        );
        if let Some(tab) = self.tabs.get(self.active) {
            let screen = tab.term.screen();
            let offset = screen.scrollback();
            let (rows, cols) = screen.size();
            let selection = tab.term.selected_range();
            for row in 0..rows {
                let content_end = screen.content_end(row);
                for col in 0..cols {
                    let cell = screen.cell(row, col).unwrap();
                    if cell.is_wide_continuation() {
                        continue;
                    }
                    let x = 8. + col as f32 * self.cw;
                    let y = 8. + row as f32 * self.ch;
                    let width = self.cw * if cell.is_wide() { 2. } else { 1. };
                    let mut foreground = self.resolve(cell.fgcolor(), tab, false);
                    let mut background = self.resolve(cell.bgcolor(), tab, true);
                    if cell.inverse() {
                        std::mem::swap(&mut foreground, &mut background);
                    }
                    let point = (row as i32 - offset as i32, col);
                    // Do not paint selection into columns with no printed text.
                    if col < content_end
                        && selection.is_some_and(|(start, end)| start <= point && point <= end)
                    {
                        background = self.selection_color();
                    }
                    if background != bg {
                        fill(
                            &mut scene,
                            Rect {
                                x,
                                y,
                                w: width,
                                h: self.ch,
                            },
                            background,
                        );
                    }
                    if cell.has_contents() {
                        text(
                            &mut scene,
                            x,
                            y,
                            cell.contents(),
                            foreground,
                            content_size,
                            cell.bold() || content_style.bold.unwrap_or(false),
                            None,
                        );
                    }
                    if cell.underline() {
                        fill(
                            &mut scene,
                            Rect {
                                x,
                                y: y + self.ch - 2.,
                                w: width,
                                h: 1.,
                            },
                            foreground,
                        );
                    }
                }
            }
            let (row, col) = screen.cursor_position();
            let x = 8. + col as f32 * self.cw;
            let y = 8. + (row as usize + offset) as f32 * self.ch;
            scene.cursor = Rect {
                x,
                y,
                w: self.cw,
                h: self.ch,
            };
            if !tab.exited && offset == 0 && !screen.hide_cursor() {
                let shape = tab.term.cursor_shape();
                let rect = match shape {
                    5 | 6 => Rect {
                        x,
                        y,
                        w: 2.,
                        h: self.ch,
                    },
                    3 | 4 => Rect {
                        x,
                        y: y + self.ch - 2.,
                        w: self.cw,
                        h: 2.,
                    },
                    _ => scene.cursor,
                };
                fill(&mut scene, rect, self.color(258));
                if shape <= 2 {
                    if let Some(cell) = screen.cell(row, col).filter(|c| c.has_contents()) {
                        text(
                            &mut scene,
                            x,
                            y,
                            cell.contents(),
                            self.cursor_text_color(),
                            content_size,
                            cell.bold() || content_style.bold.unwrap_or(false),
                            None,
                        );
                    }
                }
            }
            let history = visible_history(tab);
            if history > 0 {
                scene.scroll = Some(ScrollInfo {
                    id: "output".into(),
                    rect: Rect {
                        x: 0.,
                        y: 8.,
                        w: right,
                        h: (self.height - 16.).max(0.),
                    },
                    content: (self.height - 16.).max(0.) + history as f32 * self.ch,
                    offset: history.saturating_sub(offset) as f32 * self.ch,
                    hide_after_ms: Some(1000),
                });
            }
        }
        if let Some(error) = &self.error {
            text(
                &mut scene,
                8.,
                (self.height - 24.).max(0.),
                error.chars().take(120).collect(),
                self.ui_color("error.foreground", self.color(1)),
                error_style.size_px.unwrap(),
                error_style.bold.unwrap_or(false),
                error_style.family.clone(),
            );
        }
        scene
    }
    /// Resolve Alacritty colors with per-session OSC overrides and the user's palette.
    fn resolve(&self, color: Color, tab: &Tab, background: bool) -> u32 {
        let index = match color {
            Color::Rgb(r, g, b) => return (r as u32) << 16 | (g as u32) << 8 | b as u32,
            Color::Idx(i) => i as usize,
            Color::Default => {
                if background {
                    257
                } else {
                    256
                }
            }
        };
        tab.term
            .color_override(index)
            .unwrap_or_else(|| self.color(index))
    }
}
fn fill(scene: &mut Scene, rect: Rect, color: u32) {
    scene.paint.push(Paint::Fill {
        rect,
        color,
        extend_to_bottom: false,
    });
}
/// Allow full-height backgrounds to reach the native canvas bottom during Dock drag.
fn fill_to_bottom(scene: &mut Scene, rect: Rect, color: u32) {
    scene.paint.push(Paint::Fill {
        rect,
        color,
        extend_to_bottom: true,
    });
}
fn text(
    scene: &mut Scene,
    x: f32,
    y: f32,
    text: String,
    color: u32,
    size: f32,
    bold: bool,
    font: Option<String>,
) {
    scene.paint.push(Paint::Text {
        x,
        y,
        text,
        color,
        size,
        bold,
        font,
    });
}

/// Save presentation without shell input or private metadata.
pub(super) fn history(tab: &Tab, budget: usize) -> (String, emulator::DisplayState) {
    tab.term.snapshot_with_display(budget)
}
