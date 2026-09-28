//! Translate emulator cells into generic native drawing operations.
use super::*;
use crate::emulator::Color;
use unicode_width::UnicodeWidthChar;
/// Alacritty allocates history only when real rows leave the screen.
pub(super) fn visible_history(tab: &Tab) -> usize {
    tab.term.history()
}
impl Terminal {
    /// The host receives only drawing primitives and generic editable widget descriptions.
    pub(super) fn scene(&self) -> Scene {
        let bg = self.color(257);
        let fg = self.color(256);
        let right = self.tab_left();
        let tab_width = self.effective_tab_width();
        let mut scene = Scene {
            panel: "terminal".into(),
            font: self.settings.font_family.clone(),
            font_size: self.settings.font_size,
            ..Scene::default()
        };
        // During a drag the resize cursor follows the pointer across the panel.
        scene.column_resize_regions.push(if self.resizing_tab_bar {
            Rect {
                x: 0.,
                y: 0.,
                w: self.width,
                h: self.height,
            }
        } else {
            Rect {
                x: right,
                y: 0.,
                w: TAB_RESIZE_HANDLE_WIDTH.min(tab_width),
                h: self.height,
            }
        });
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
        fill_to_bottom(
            &mut scene,
            Rect {
                x: right,
                y: 0.,
                w: tab_width,
                h: self.height,
            },
            self.env.muted,
        );
        for (i, tab) in self.tabs.iter().enumerate().skip(self.tab_scroll) {
            let y = (i - self.tab_scroll) as f32 * 32.;
            if y >= self.height {
                break;
            }
            if i == self.active {
                fill(
                    &mut scene,
                    Rect {
                        x: right,
                        y,
                        w: tab_width,
                        h: 32.,
                    },
                    bg,
                );
            } else {
                fill(
                    &mut scene,
                    Rect {
                        x: right,
                        y,
                        w: 1.,
                        h: 32.,
                    },
                    self.env.border,
                );
            }
            if self.rename != Some(tab.id) {
                let label = format!("{}{}", tab.name, if tab.exited { " · 已退出" } else { "" });
                // Reserve the close-button area and shorten only the painted label.
                let label_cell_width = (self.cw * 14. / self.settings.font_size).max(1.);
                let label_cells = ((tab_width - 38.) / label_cell_width).floor().max(0.) as usize;
                text(
                    &mut scene,
                    right + 8.,
                    y + 5.,
                    tab_display_name(&label, label_cells),
                    fg,
                    14.,
                    false,
                );
                // Hide the close target while the native input owns the whole tab.
                fill(
                    &mut scene,
                    Rect {
                        x: self.width - 30.,
                        y,
                        w: 30.,
                        h: 31.,
                    },
                    if i == self.active { bg } else { self.env.muted },
                );
                text(
                    &mut scene,
                    self.width - 22.,
                    y + 5.,
                    "×".into(),
                    fg,
                    14.,
                    false,
                );
            }
            fill(
                &mut scene,
                Rect {
                    x: right,
                    y: y + 31.,
                    w: tab_width,
                    h: 1.,
                },
                self.env.border,
            );
            if self.rename == Some(tab.id) {
                scene.widgets.push(Widget {
                    id: format!("rename:{}", tab.id),
                    // Native editing replaces the entire tab until focus leaves the input.
                    rect: Rect {
                        x: right,
                        y,
                        w: tab_width,
                        h: 32.,
                    },
                    label: tab.name.clone(),
                    edit: true,
                });
            }
        }
        let below = self.tabs.len().saturating_sub(self.tab_scroll) as f32 * 32.;
        fill_to_bottom(
            &mut scene,
            Rect {
                x: right,
                y: below,
                w: 1.,
                h: (self.height - below).max(0.),
            },
            self.env.border,
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
                            self.settings.font_size,
                            cell.bold(),
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
                            self.settings.font_size,
                            cell.bold(),
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
        if self.menu {
            let actions = self.menu_actions();
            fill(
                &mut scene,
                Rect {
                    x: 0.,
                    y: 0.,
                    w: 230.,
                    h: actions.len() as f32 * 28. + 8.,
                },
                self.env.muted,
            );
            for (i, (_, label)) in actions.iter().skip(self.menu_scroll).enumerate() {
                if i as f32 * 28. >= self.height {
                    break;
                }
                text(
                    &mut scene,
                    8.,
                    4. + i as f32 * 28.,
                    label.clone(),
                    fg,
                    14.,
                    false,
                );
            }
        }
        if let Some(error) = &self.error {
            text(
                &mut scene,
                8.,
                (self.height - 24.).max(0.),
                error.chars().take(120).collect(),
                self.color(1),
                13.,
                false,
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
/// Fit a tab label by Unicode display cells, retaining the full name for editing.
fn tab_display_name(label: &str, max_cells: usize) -> String {
    let total: usize = label.chars().map(|c| c.width().unwrap_or(0)).sum();
    if total <= max_cells {
        return label.to_owned();
    }
    if max_cells == 0 {
        return String::new();
    }
    let mut result = String::new();
    let mut used = 0;
    for ch in label.chars() {
        let width = ch.width().unwrap_or(0);
        if used + width >= max_cells {
            break;
        }
        result.push(ch);
        used += width;
    }
    result.push('…');
    result
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
fn text(scene: &mut Scene, x: f32, y: f32, text: String, color: u32, size: f32, bold: bool) {
    scene.paint.push(Paint::Text {
        x,
        y,
        text,
        color,
        size,
        bold,
    });
}

/// Save presentation without shell input or private metadata.
pub(super) fn history(tab: &Tab, budget: usize) -> String {
    tab.term.snapshot(budget)
}
