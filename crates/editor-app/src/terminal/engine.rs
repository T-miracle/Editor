//! Upstream Alacritty owns VT parsing, grids, history and selection; native presentation reads it.

use alacritty_terminal::{
    Term,
    event::{Event, EventListener},
    grid::{Dimensions, Scroll},
    index::{Column, Line, Point, Side},
    selection::{Selection, SelectionType},
    term::{
        Config, TermMode,
        cell::{Cell, Flags},
    },
    vte::{
        ansi::Processor,
        ansi::{ClearMode, Color, CursorShape, CursorStyle, Handler, NamedColor},
    },
};
use plugin_runtime::plugin_protocol::{
    self as protocol, Paint, Rect,
    ui::{Canvas, ScrollRange},
};
use rust_i18n::t;
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    rc::Rc,
};

mod recovery;

#[cfg(test)]
mod tests;

/// Actual character dimensions, independent from dock and sidebar pixels.
#[derive(Clone, Copy)]
pub(super) struct GridSize {
    pub columns: usize,
    pub rows: usize,
}
impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.columns
    }
}

/// Reply events stay on the view thread; process writes are queued to the native supervisor.
#[derive(Clone, Default)]
struct Listener(Rc<RefCell<Vec<Event>>>);
impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        self.0.borrow_mut().push(event);
    }
}

/// Saved logical cells contain no OS resources, executable state or debugger memory.
#[derive(Serialize, Deserialize)]
pub(super) struct SavedGrid {
    pub columns: usize,
    pub rows: usize,
    /// Trim trailing default cells, never actual content; history is oldest-first and bounded.
    pub lines: VecDeque<Vec<Cell>>,
    pub cursor: (i32, usize),
    pub input_needs_wrap: bool,
    pub offset: usize,
}

pub(super) struct Engine {
    term: Term<Listener>,
    parser: Processor,
    listener: Listener,
    history_limit: usize,
    /// Only ConPTY's first inheritance query may reuse an empty restored prompt.
    bootstrap: Option<Vec<u8>>,
    bootstrap_line_start: bool,
    /// ConPTY owns a fresh screen; historical rows must not be pulled into it during resize.
    conpty_screen: bool,
    /// Explicit history scrolling pauses following; user input or returning to the bottom resumes it.
    follow_screen: bool,
    metadata_parser: alacritty_terminal::vte::Parser,
    metadata: super::shell::Metadata,
}

/// Editor defaults and user overrides are resolved once per frame, outside the cell loop.
pub(super) struct Colors {
    pub background: u32,
    pub foreground: u32,
    pub cursor: u32,
    pub selection: u32,
    pub ansi: [u32; 16],
    pub overrides: BTreeMap<usize, u32>,
}

impl Engine {
    /// Styled spaces, wide-glyph spacers and wrap metadata belong to retained output too.
    fn has_content(cell: &Cell) -> bool {
        cell != &Cell::default()
    }

    /// Translate a visible cell to native screen coordinates, rejecting retained history rows.
    pub fn mouse_position(&self, row: usize, column: usize) -> Option<(usize, usize)> {
        let row = row.checked_sub(self.offset())?;
        (row < self.term.screen_lines() && column < self.term.columns()).then_some((row, column))
    }

    pub fn new(size: GridSize, history: usize) -> Self {
        let listener = Listener::default();
        let config = Config {
            scrolling_history: history,
            default_cursor_style: CursorStyle {
                shape: CursorShape::Beam,
                blinking: false,
            },
            ..Config::default()
        };
        Self {
            term: Term::new(config, &size, listener.clone()),
            parser: Processor::new(),
            listener,
            history_limit: history,
            bootstrap: None,
            bootstrap_line_start: false,
            conpty_screen: false,
            follow_screen: true,
            metadata_parser: alacritty_terminal::vte::Parser::new(),
            metadata: Default::default(),
        }
    }
    /// Parsing never touches native handles or UI widgets; replies are returned to the owner.
    pub fn process(&mut self, bytes: &[u8]) -> Vec<Event> {
        const QUERY: &[u8] = b"\x1b[6n";
        let buffered = if let Some(mut initial) = self.bootstrap.take() {
            initial.extend_from_slice(bytes);
            if initial.len() < QUERY.len() && QUERY.starts_with(&initial) {
                self.bootstrap = Some(initial);
                return vec![];
            }
            if initial.starts_with(QUERY) {
                let start = self.start_restored_screen();
                self.listener
                    .send_event(Event::PtyWrite(format!("\x1b[{};1R", start.0 + 1)));
                self.bootstrap_line_start = true;
                Some(initial.split_off(QUERY.len()))
            } else {
                Some(initial)
            }
        } else {
            None
        };
        let bytes = buffered.as_deref().unwrap_or(bytes);
        if self.bootstrap_line_start && !bytes.is_empty() {
            // ConPTY redraw starts at the logical prompt's first row, not its last wrapped row.
            // Measurement may have reflowed rows while native startup was pending. Resolve the
            // current start again rather than retaining a stale pre-resize screen coordinate.
            let start = self.visible_prompt_start();
            let cursor = &mut self.term.grid_mut().cursor;
            cursor.point = Point::new(start, Column(0));
            cursor.input_needs_wrap = false;
            self.bootstrap_line_start = false;
        }
        self.parser.advance(&mut self.term, bytes);
        self.metadata_parser.advance(&mut self.metadata, bytes);
        self.reveal_native_screen();
        self.listener.0.borrow_mut().drain(..).collect()
    }
    /// Directory metadata never changes the grid and cannot execute a program itself.
    pub fn take_cwd(&mut self) -> Option<String> {
        self.metadata.cwd.take()
    }
    pub fn resize(&mut self, size: GridSize) {
        if self.conpty_screen
            && self.bootstrap.is_none()
            && !self.term.mode().contains(TermMode::ALT_SCREEN)
        {
            self.resize_conpty_screen(size);
            return;
        }
        self.term.resize(size);
    }

    /// Once native EOF is observed, no future redraw can restore clipped screen cells. Ended tabs
    /// therefore return to Alacritty's full logical reflow before any subsequent geometry change.
    pub fn end_process(&mut self) {
        self.conpty_screen = false;
        self.bootstrap = None;
        self.bootstrap_line_start = false;
    }

    /// History settings update Alacritty's limit without resetting the active screen or mode.
    pub fn set_history(&mut self, history: usize) {
        self.history_limit = history;
        self.term.set_options(Config {
            scrolling_history: history,
            default_cursor_style: CursorStyle {
                shape: CursorShape::Beam,
                blinking: false,
            },
            ..Config::default()
        });
    }
    pub fn mode(&self) -> TermMode {
        *self.term.mode()
    }
    pub fn history(&self) -> usize {
        self.term.grid().history_size()
    }
    pub fn offset(&self) -> usize {
        self.term.grid().display_offset()
    }
    pub fn scroll(&mut self, lines: i32) {
        self.term.scroll_display(Scroll::Delta(lines));
        self.follow_screen = self.offset() == 0;
    }
    pub fn set_offset(&mut self, offset: usize) {
        self.follow_screen = offset == 0;
        self.term
            .scroll_display(Scroll::Delta(offset as i32 - self.offset() as i32));
    }
    pub fn clear(&mut self) {
        // Clear primary history as well when a TUI is active, retaining its input/mouse modes.
        let alternate = self.term.mode().contains(TermMode::ALT_SCREEN);
        if alternate {
            self.term.swap_alt();
        }
        self.term.clear_screen(ClearMode::All);
        self.term.clear_screen(ClearMode::Saved);
        self.term.grid_mut().cursor.point = Point::new(Line(0), Column(0));
        self.term.grid_mut().cursor.input_needs_wrap = false;
        if alternate {
            self.term.swap_alt();
        }
        self.term.selection = None;
        self.set_offset(0);
    }
    /// Save public logical cells, rather than depending on Alacritty's private storage layout.
    pub fn snapshot(&self) -> SavedGrid {
        let grid = self.term.grid();
        let lines = (-(grid.history_size() as i32)..grid.screen_lines() as i32)
            .map(|line| {
                let mut cells = (0..grid.columns())
                    .map(|column| grid[Point::new(Line(line), Column(column))].clone())
                    .collect::<Vec<_>>();
                while cells.last() == Some(&Cell::default()) {
                    cells.pop();
                }
                cells
            })
            .collect();
        SavedGrid {
            columns: grid.columns(),
            rows: grid.screen_lines(),
            lines,
            cursor: (grid.cursor.point.line.0, grid.cursor.point.column.0),
            input_needs_wrap: grid.cursor.input_needs_wrap,
            offset: grid.display_offset(),
        }
    }
    /// Validate geometry before allocating or indexing; a damaged snapshot cannot panic the UI.
    pub fn restore(&mut self, saved: SavedGrid) -> anyhow::Result<()> {
        anyhow::ensure!(
            (2..=1000).contains(&saved.columns) && (1..=500).contains(&saved.rows),
            t!("terminal.invalid_snapshot")
        );
        anyhow::ensure!(
            saved.lines.len() >= saved.rows
                && saved.lines.len() <= saved.rows + self.history_limit
                && saved.lines.iter().all(|row| row.len() <= saved.columns),
            t!("terminal.invalid_snapshot")
        );
        anyhow::ensure!(
            (0..saved.rows as i32).contains(&saved.cursor.0) && saved.cursor.1 < saved.columns,
            t!("terminal.invalid_snapshot")
        );
        let history = saved.lines.len() - saved.rows;
        let grid = self.term.grid_mut();
        // A snapshot replaces cells rather than overlaying trimmed rows on a previous screen.
        // Construct history with the default rendition: scroll_up also initializes blank suffixes
        // in history. Restoring the live template earlier would tint old rows with the new SGR.
        // Keep live rendition/saved cursor for subsequent output, after all saved cells are copied.
        let template = grid.cursor.template.clone();
        let saved_cursor = grid.saved_cursor.clone();
        grid.reset();
        grid.scroll_up::<Color>(&(Line(0)..Line(saved.rows as i32)), history);
        for (row, cells) in saved.lines.into_iter().enumerate() {
            for (column, cell) in cells.into_iter().enumerate() {
                grid[Point::new(Line(row as i32 - history as i32), Column(column))] = cell;
            }
        }
        grid.cursor.template = template;
        grid.saved_cursor = saved_cursor;
        grid.cursor.point = Point::new(Line(saved.cursor.0), Column(saved.cursor.1));
        grid.cursor.input_needs_wrap = saved.input_needs_wrap;
        grid.scroll_display(Scroll::Delta(saved.offset.min(history) as i32));
        Ok(())
    }
    pub fn selected_text(&self) -> Option<String> {
        self.term
            .selection_to_string()
            .filter(|text| !text.is_empty())
    }
    pub fn clear_selection(&mut self) {
        self.term.selection = None;
    }

    /// Ignore padding after the last printed cell, while allowing spaces inside actual content.
    pub fn select(&mut self, row: usize, column: usize, start: bool, clicks: u8) {
        let line = Line(row as i32 - self.offset() as i32);
        let last = (0..self.term.columns()).rev().find(|&column| {
            let cell = &self.term.grid()[Point::new(line, Column(column))];
            cell.c != ' '
                || cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                || cell.zerowidth().is_some_and(|text| !text.is_empty())
        });
        if start && last.is_none_or(|last| column > last) {
            self.clear_selection();
            return;
        }
        let point = Point::new(
            line,
            Column(
                column
                    .min(last.map_or(0, |last| last + 1))
                    .min(self.term.columns() - 1),
            ),
        );
        if start {
            let kind = match clicks {
                2 => SelectionType::Semantic,
                3.. => SelectionType::Lines,
                _ => SelectionType::Simple,
            };
            self.term.selection = Some(Selection::new(kind, point, Side::Left));
        } else if let Some(selection) = &mut self.term.selection {
            selection.update(point, Side::Right);
        }
    }

    /// Produce native drawing data directly; no Document tree, serialization or guest event loop.
    pub fn drawing(
        &self,
        width: f32,
        height: f32,
        cell_width: f32,
        cell_height: f32,
        font: &str,
        font_size: f32,
        colors: &Colors,
    ) -> Canvas {
        let mut canvas = Canvas {
            font: protocol::FontStyle {
                family: Some(font.into()),
                size_px: Some(font_size),
                bold: None,
            },
            focusable: true,
            grid: true,
            ..Default::default()
        };
        let rect = |x, y, w, h| Rect { x, y, w, h };
        canvas.paint.push(Paint::Fill {
            rect: rect(0., 0., width, height),
            color: colors.background,
            extend_to_bottom: true,
        });
        let content = self.term.renderable_content();
        for item in self.term.grid().display_iter() {
            let cell = item.cell;
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }
            let row = item.point.line.0 + self.offset() as i32;
            if row < 0 || row as usize >= self.term.screen_lines() {
                continue;
            }
            let x = 8. + item.point.column.0 as f32 * cell_width;
            let y = 8. + row as f32 * cell_height;
            let mut fg = self.color(cell.fg, colors, cell.flags.contains(Flags::BOLD));
            if cell.flags.contains(Flags::DIM) {
                let index = match cell.fg {
                    Color::Named(NamedColor::Foreground) => Some(268),
                    Color::Named(color) if (color as usize) < 8 => Some(color as usize + 259),
                    Color::Indexed(index @ 0..=7) => Some(index as usize + 259),
                    _ => None,
                };
                if let Some(index) = index {
                    fg = self.color_index(index, colors);
                }
            }
            let mut bg = self.color(cell.bg, colors, false);
            if cell.flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }
            if content
                .selection
                .as_ref()
                .is_some_and(|selection| selection.contains(item.point))
            {
                bg = colors.selection;
            }
            let span = if cell.flags.contains(Flags::WIDE_CHAR) {
                2.
            } else {
                1.
            };
            if bg != colors.background {
                canvas.paint.push(Paint::Fill {
                    rect: rect(x, y, span * cell_width, cell_height),
                    color: bg,
                    extend_to_bottom: false,
                });
            }
            if cell.c != ' ' && !cell.flags.contains(Flags::HIDDEN) {
                let mut text = cell.c.to_string();
                if let Some(extra) = cell.zerowidth() {
                    text.extend(extra);
                }
                canvas.paint.push(Paint::Text {
                    x,
                    y,
                    text,
                    color: fg,
                    size: font_size,
                    bold: cell.flags.contains(Flags::BOLD),
                    font: Some(font.into()),
                });
            }
            if cell.flags.intersects(
                Flags::UNDERLINE
                    | Flags::DOUBLE_UNDERLINE
                    | Flags::UNDERCURL
                    | Flags::DOTTED_UNDERLINE
                    | Flags::DASHED_UNDERLINE,
            ) {
                canvas.paint.push(Paint::Fill {
                    rect: rect(x, y + cell_height - 2., span * cell_width, 1.),
                    color: fg,
                    extend_to_bottom: false,
                });
            }
            if cell.flags.contains(Flags::STRIKEOUT) {
                canvas.paint.push(Paint::Fill {
                    rect: rect(x, y + cell_height / 2., span * cell_width, 1.),
                    color: fg,
                    extend_to_bottom: false,
                });
            }
        }
        // The restored prefix can be visible above the new native screen. Map its caret to the
        // viewport without altering the coordinates sent to the PTY.
        if self.term.mode().contains(TermMode::SHOW_CURSOR)
            && let Some(cursor) = alacritty_terminal::term::point_to_viewport(
                self.offset(),
                self.term.grid().cursor.point,
            )
            && cursor.line < self.term.screen_lines()
        {
            let caret = rect(
                8. + cursor.column.0 as f32 * cell_width,
                8. + cursor.line as f32 * cell_height,
                1.5,
                cell_height,
            );
            canvas.paint.push(Paint::Fill {
                rect: caret,
                color: colors.cursor,
                extend_to_bottom: false,
            });
            canvas.caret = Some(caret);
        }
        if self.history() > 0 {
            canvas.scroll = Some(ScrollRange {
                content: height + self.history() as f32 * cell_height,
                offset: self.history().saturating_sub(self.offset()) as f32 * cell_height,
            });
        }
        canvas
    }

    fn color(&self, value: Color, colors: &Colors, bold: bool) -> u32 {
        let index = match value {
            Color::Spec(color) => {
                return u32::from(color.r) << 16 | u32::from(color.g) << 8 | u32::from(color.b);
            }
            Color::Indexed(index) => index as usize,
            Color::Named(color) => color as usize,
        };
        let index = if bold && index < 8 {
            index + 8
        } else if bold && index == 256 {
            267
        } else {
            index
        };
        self.color_index(index, colors)
    }

    /// The same effective color answers OSC queries and paints cells, including runtime overrides.
    pub fn color_index(&self, index: usize, colors: &Colors) -> u32 {
        if index < 269
            && let Some(color) = self.term.colors()[index]
        {
            return u32::from(color.r) << 16 | u32::from(color.g) << 8 | u32::from(color.b);
        }
        if let Some(color) = colors.overrides.get(&index) {
            return *color;
        }
        match index {
            0..=15 => colors.ansi[index],
            16..=231 => {
                let n = index - 16;
                let level = |value: usize| if value == 0 { 0 } else { 55 + value * 40 };
                ((level(n / 36) << 16) | (level(n / 6 % 6) << 8) | level(n % 6)) as u32
            }
            232..=255 => {
                let gray = (8 + (index - 232) * 10) as u32;
                gray << 16 | gray << 8 | gray
            }
            value if value == NamedColor::Background as usize => colors.background,
            258 => colors.cursor,
            259..=266 => colors.ansi[index - 259],
            _ => colors.foreground,
        }
    }
}
