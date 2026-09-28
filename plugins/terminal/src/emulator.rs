//! The terminal guest owns Alacritty's VT parser, cells, modes and scrollback.
use alacritty_terminal::Term;
use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::{Dimensions, Grid, Scroll};
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::{Config, TermMode};
use alacritty_terminal::vte::ansi::{
    Color as CoreColor, CursorShape, CursorStyle, NamedColor, Processor, Rgb,
};
use std::cell::{RefCell, RefMut};
use std::collections::VecDeque;
use std::rc::Rc;

/// Color representation kept at the scene/snapshot boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Color {
    Default,
    Idx(u16),
    Rgb(u8, u8, u8),
}

/// Convert Alacritty's named, indexed and true colors to a palette-independent value.
fn color(value: CoreColor) -> Color {
    match value {
        CoreColor::Spec(rgb) => Color::Rgb(rgb.r, rgb.g, rgb.b),
        CoreColor::Indexed(index) => Color::Idx(index as u16),
        CoreColor::Named(NamedColor::Foreground | NamedColor::Background | NamedColor::Cursor) => {
            Color::Default
        }
        // Keep 259..268 intact: clamping named dim colors to 255 made them pale gray.
        CoreColor::Named(named) => Color::Idx(named as u16),
    }
}

/// Host-facing answers and theme values, shared with Alacritty's event callback.
#[derive(Default)]
pub(super) struct Replies {
    pub bytes: Vec<u8>,
    pub palette: Vec<u32>,
    pub colors: std::collections::BTreeMap<usize, u32>,
    pub cell_size: (u16, u16),
    pub grid_size: (u16, u16),
}

/// Convert parser events into PTY replies without giving the WASM guest direct OS access.
#[derive(Clone)]
struct Listener(Rc<RefCell<Replies>>);
impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        let mut replies = self.0.borrow_mut();
        match event {
            Event::PtyWrite(text) => replies.bytes.extend_from_slice(text.as_bytes()),
            Event::ColorRequest(index, format) => {
                let rgb = replies
                    .colors
                    .get(&index)
                    .copied()
                    .or_else(|| replies.palette.get(index).copied())
                    .unwrap_or(0);
                replies.bytes.extend_from_slice(
                    format(Rgb {
                        r: (rgb >> 16) as u8,
                        g: (rgb >> 8) as u8,
                        b: rgb as u8,
                    })
                    .as_bytes(),
                );
            }
            Event::TextAreaSizeRequest(format) => {
                let (cell_width, cell_height) = replies.cell_size;
                let (num_lines, num_cols) = replies.grid_size;
                let size = WindowSize {
                    num_lines,
                    num_cols,
                    cell_width,
                    cell_height,
                };
                replies.bytes.extend_from_slice(format(size).as_bytes());
            }
            _ => {}
        }
    }
}

/// Alacritty accepts any type implementing its generic viewport dimensions.
struct Size {
    rows: usize,
    cols: usize,
}
impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

/// Mouse modes are translated once for the plugin interaction layer.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum MouseProtocolMode {
    None,
    Press,
    ButtonMotion,
    AnyMotion,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum MouseProtocolEncoding {
    Legacy,
    Sgr,
}

/// Read-only viewport over Alacritty's grid, including a scrolled history region.
pub(super) struct Screen<'a> {
    grid: &'a Grid<Cell>,
    mode: TermMode,
}
impl<'a> Screen<'a> {
    pub(super) fn size(&self) -> (u16, u16) {
        (self.grid.screen_lines() as u16, self.grid.columns() as u16)
    }
    pub(super) fn scrollback(&self) -> usize {
        self.grid.display_offset()
    }
    pub(super) fn alternate_screen(&self) -> bool {
        self.mode.contains(TermMode::ALT_SCREEN)
    }
    pub(super) fn application_cursor(&self) -> bool {
        self.mode.contains(TermMode::APP_CURSOR)
    }
    pub(super) fn bracketed_paste(&self) -> bool {
        self.mode.contains(TermMode::BRACKETED_PASTE)
    }
    pub(super) fn hide_cursor(&self) -> bool {
        !self.mode.contains(TermMode::SHOW_CURSOR)
    }
    pub(super) fn cursor_position(&self) -> (u16, u16) {
        (
            self.grid.cursor.point.line.0.max(0) as u16,
            self.grid.cursor.point.column.0 as u16,
        )
    }
    pub(super) fn mouse_protocol_mode(&self) -> MouseProtocolMode {
        if self.mode.contains(TermMode::MOUSE_MOTION) {
            MouseProtocolMode::AnyMotion
        } else if self.mode.contains(TermMode::MOUSE_DRAG) {
            MouseProtocolMode::ButtonMotion
        } else if self.mode.contains(TermMode::MOUSE_REPORT_CLICK) {
            MouseProtocolMode::Press
        } else {
            MouseProtocolMode::None
        }
    }
    pub(super) fn mouse_protocol_encoding(&self) -> MouseProtocolEncoding {
        if self.mode.contains(TermMode::SGR_MOUSE) {
            MouseProtocolEncoding::Sgr
        } else {
            MouseProtocolEncoding::Legacy
        }
    }
    pub(super) fn cell(&self, row: u16, col: u16) -> Option<CellView<'a>> {
        if row as usize >= self.grid.screen_lines() {
            return None;
        }
        self.cell_line(row as i32 - self.scrollback() as i32, col)
    }
    /// Exclude unused terminal columns while retaining spaces inside actual line content.
    pub(super) fn content_end(&self, row: u16) -> u16 {
        (0..self.size().1)
            .rev()
            .find_map(|col| {
                self.cell(row, col)
                    .filter(|cell| cell.has_contents())
                    .map(|cell| col + if cell.is_wide() { 2 } else { 1 })
            })
            .unwrap_or(0)
            .min(self.size().1)
    }
    pub(super) fn cell_line(&self, line: i32, col: u16) -> Option<CellView<'a>> {
        if col as usize >= self.grid.columns()
            || line < -(self.grid.history_size() as i32)
            || line >= self.grid.screen_lines() as i32
        {
            return None;
        }
        Some(CellView(
            &self.grid[Point::new(Line(line), Column(col as usize))],
        ))
    }
    pub(super) fn row_wrapped(&self, line: i32) -> bool {
        self.cell_line(line, self.size().1.saturating_sub(1))
            .is_some_and(|cell| cell.0.flags.contains(Flags::WRAPLINE))
    }
    #[cfg(test)]
    pub(super) fn contents(&self) -> String {
        (0..self.size().0)
            .map(|row| {
                let mut text = String::new();
                for col in 0..self.size().1 {
                    if let Some(cell) = self.cell(row, col) {
                        if !cell.is_wide_continuation() {
                            text.push_str(&cell.contents());
                        }
                    }
                }
                text.trim_end().to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Expose cell styling without moving rendering policy into Alacritty.
pub(super) struct CellView<'a>(&'a Cell);
impl CellView<'_> {
    pub(super) fn contents(&self) -> String {
        let mut text = self.0.c.to_string();
        if let Some(extra) = self.0.zerowidth() {
            text.extend(extra.iter());
        }
        text
    }
    pub(super) fn has_contents(&self) -> bool {
        self.0.c != ' ' || self.0.zerowidth().is_some_and(|chars| !chars.is_empty())
    }
    pub(super) fn is_wide_continuation(&self) -> bool {
        self.0.flags.contains(Flags::WIDE_CHAR_SPACER)
    }
    pub(super) fn is_wide(&self) -> bool {
        self.0.flags.contains(Flags::WIDE_CHAR)
    }
    pub(super) fn fgcolor(&self) -> Color {
        let foreground = color(self.0.fg);
        // The terminal core stores SGR intensity as flags; resolve named colors for painting.
        match foreground {
            Color::Default if self.0.flags.contains(Flags::DIM) => Color::Idx(268),
            Color::Default if self.0.flags.contains(Flags::BOLD) => Color::Idx(267),
            Color::Idx(index @ 0..=7) if self.0.flags.contains(Flags::DIM) => {
                Color::Idx(index + 259)
            }
            Color::Idx(index @ 0..=7) if self.0.flags.contains(Flags::BOLD) => {
                Color::Idx(index + 8)
            }
            _ => foreground,
        }
    }
    pub(super) fn bgcolor(&self) -> Color {
        color(self.0.bg)
    }
    pub(super) fn bold(&self) -> bool {
        self.0.flags.contains(Flags::BOLD)
    }
    pub(super) fn underline(&self) -> bool {
        self.0.flags.intersects(Flags::ALL_UNDERLINES)
    }
    pub(super) fn inverse(&self) -> bool {
        self.0.flags.contains(Flags::INVERSE)
    }
}

/// Selection points use live viewport coordinates, so scrolling does not move the selection.
#[derive(Clone, Copy)]
struct Selection {
    anchor: (i32, u16),
    end: (i32, u16),
    clicks: u8,
}

/// Single terminal session: upstream parser and grid plus editor-facing presentation state.
pub(super) struct Emulator {
    term: Term<Listener>,
    parser: Processor,
    replies: Rc<RefCell<Replies>>,
    selection: Option<Selection>,
}
impl Emulator {
    /// Construct an Alacritty screen with bounded history.
    pub fn new(rows: u16, cols: u16, limit: usize) -> Self {
        let replies = Rc::new(RefCell::new(Replies {
            palette: vec![0; 269],
            cell_size: (8, 21),
            grid_size: (rows, cols),
            ..Replies::default()
        }));
        let config = Config {
            scrolling_history: limit,
            // A beam is the plugin default; explicit Shell/TUI cursor sequences still win.
            default_cursor_style: CursorStyle {
                shape: CursorShape::Beam,
                blinking: false,
            },
            ..Config::default()
        };
        let size = Size {
            rows: rows.max(1) as usize,
            cols: cols.max(2) as usize,
        };
        Self {
            term: Term::new(config, &size, Listener(replies.clone())),
            parser: Processor::new(),
            replies,
            selection: None,
        }
    }
    /// Borrow the active viewport for rendering and input mode checks.
    pub fn screen(&self) -> Screen<'_> {
        Screen {
            grid: self.term.grid(),
            mode: *self.term.mode(),
        }
    }
    /// Set cell dimensions or drain replies from the event loop.
    pub fn replies_mut(&self) -> RefMut<'_, Replies> {
        self.replies.borrow_mut()
    }
    /// Feed PTY output to Alacritty; no output is sent back as shell input.
    pub fn process(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
        // Keep OSC palette changes available to subsequent color-query callbacks.
        let mut replies = self.replies.borrow_mut();
        replies.colors.clear();
        for index in 0..269 {
            if let Some(rgb) = self.term.colors()[index] {
                replies.colors.insert(
                    index,
                    (rgb.r as u32) << 16 | (rgb.g as u32) << 8 | rgb.b as u32,
                );
            }
        }
        self.selection = None;
    }
    /// Resize the emulator grid without introducing a line of shell input.
    pub fn resize(&mut self, rows: u16, cols: u16) {
        self.term.resize(Size {
            rows: rows.max(1) as usize,
            cols: cols.max(2) as usize,
        });
        self.replies.borrow_mut().grid_size = (rows, cols);
        self.selection = None;
    }
    /// Return real primary history, excluding alternate-screen contents.
    pub fn history(&self) -> usize {
        if self.screen().alternate_screen() {
            0
        } else {
            self.term.grid().history_size()
        }
    }
    /// Scroll toward older output for positive deltas.
    pub fn scroll(&mut self, delta: i32) {
        self.term.scroll_display(Scroll::Delta(delta));
    }
    /// Set an absolute history offset for the native scrollbar.
    pub fn set_scrollback(&mut self, offset: usize) {
        let delta = offset as i32 - self.term.grid().display_offset() as i32;
        self.scroll(delta);
    }
    /// Apply the plugin-configured scrollback cap to the primary grid.
    pub fn set_history_limit(&mut self, limit: usize) {
        self.term.primary_grid_mut().update_history(limit);
    }
    /// Remove old lines without erasing the visible terminal screen.
    pub fn clear_history(&mut self) {
        self.term.primary_grid_mut().clear_history();
        self.selection = None;
    }
    /// Begin character, word or line selection.
    pub fn select(&mut self, row: u16, col: u16, clicks: u8) {
        let point = (row as i32 - self.screen().scrollback() as i32, col);
        self.selection = Some(Selection {
            anchor: point,
            end: point,
            clicks,
        });
    }
    /// A press outside printed content removes an old selection without creating a new one.
    pub fn clear_selection(&mut self) {
        self.selection = None;
    }
    /// Extend a selection while preserving its history-relative anchor.
    pub fn extend_selection(&mut self, row: u16, col: u16) {
        let offset = self.screen().scrollback() as i32;
        if let Some(selection) = &mut self.selection {
            selection.end = (row as i32 - offset, col);
        }
    }
    /// Expand double- and triple-click selections around words and full lines.
    fn selection_bounds(&self) -> Option<((i32, u16), (i32, u16))> {
        let selection = self.selection?;
        let (mut start, mut end) = if selection.anchor <= selection.end {
            (selection.anchor, selection.end)
        } else {
            (selection.end, selection.anchor)
        };
        let screen = self.screen();
        let cols = screen.size().1;
        if selection.clicks >= 3 {
            start.1 = 0;
            end.1 = cols - 1;
        } else if selection.clicks == 2 {
            for (point, left) in [(&mut start, true), (&mut end, false)] {
                let word = |col| {
                    screen.cell_line(point.0, col).is_some_and(|cell| {
                        cell.contents()
                            .chars()
                            .any(|c| c.is_alphanumeric() || c == '_')
                    })
                };
                if word(point.1) {
                    if left {
                        while point.1 > 0 && word(point.1 - 1) {
                            point.1 -= 1;
                        }
                    } else {
                        while point.1 + 1 < cols && word(point.1 + 1) {
                            point.1 += 1;
                        }
                    }
                }
            }
        }
        Some((start, end))
    }
    /// Return bounds once per frame for selection highlighting.
    pub fn selected_range(&self) -> Option<((i32, u16), (i32, u16))> {
        self.selection_bounds()
    }
    /// Extract only selected text, preserving hard line endings.
    pub fn selection_text(&self) -> Option<String> {
        let (start, end) = self.selection_bounds()?;
        let screen = self.screen();
        let cols = screen.size().1;
        let mut text = String::new();
        for line in start.0..=end.0 {
            let first = if line == start.0 { start.1 } else { 0 };
            let last = if line == end.0 { end.1 } else { cols - 1 };
            let mut value = String::new();
            for col in first..=last {
                if let Some(cell) = screen.cell_line(line, col) {
                    if !cell.is_wide_continuation() {
                        value.push_str(&cell.contents());
                    }
                }
            }
            text.push_str(value.trim_end());
            if line < end.0 && !screen.row_wrapped(line) {
                text.push('\n');
            }
        }
        Some(text)
    }
    /// Save only styled primary-screen output, compatible with the existing ANSI snapshot schema.
    pub fn snapshot(&self, budget: usize) -> String {
        let grid = self.term.primary_grid();
        let rows = grid.screen_lines();
        let last = (0..rows)
            .rev()
            .find(|row| snapshot_line_end(grid, *row as i32) > 0)
            .unwrap_or(0);
        let mut lines = VecDeque::new();
        let mut bytes = 0;
        // Traverse newest lines first and stop as soon as the storage budget is full.
        for line in (-(grid.history_size() as i32)..=last as i32).rev() {
            let mut value = String::from("\x1b[0m");
            let row = &grid[Line(line)];
            let end = snapshot_line_end(grid, line);
            let mut previous = None;
            for col in 0..end {
                let cell = &row[Column(col)];
                if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    continue;
                }
                let style = (cell.fg, cell.bg, cell.flags);
                if previous != Some(style) {
                    value.push_str("\x1b[0m");
                    ansi_color(&mut value, color(cell.fg), false);
                    ansi_color(&mut value, color(cell.bg), true);
                    for (enabled, code) in [
                        (cell.flags.contains(Flags::BOLD), 1),
                        (cell.flags.contains(Flags::DIM), 2),
                        (cell.flags.contains(Flags::ITALIC), 3),
                        (cell.flags.intersects(Flags::ALL_UNDERLINES), 4),
                        (cell.flags.contains(Flags::INVERSE), 7),
                    ] {
                        if enabled {
                            value.push_str(&format!("\x1b[{code}m"));
                        }
                    }
                    previous = Some(style);
                }
                // Append directly so a long line does not allocate once per character.
                value.push(cell.c);
                if let Some(extra) = cell.zerowidth() {
                    value.extend(extra.iter());
                }
            }
            value.push_str("\x1b[0m\r\n");
            if value.len() > budget {
                continue;
            }
            if bytes + value.len() > budget {
                break;
            }
            bytes += value.len();
            lines.push_front(value);
        }
        lines.into_iter().collect()
    }
    /// Resolve dynamic OSC palette values retained by Alacritty.
    pub fn color_override(&self, index: usize) -> Option<u32> {
        self.term.colors()[index]
            .map(|rgb| (rgb.r as u32) << 16 | (rgb.g as u32) << 8 | rgb.b as u32)
    }
    /// Expose the active focus-reporting mode.
    pub fn focus_mode(&self) -> bool {
        self.term.mode().contains(TermMode::FOCUS_IN_OUT)
    }
    /// Return the cursor shape selected by shell or TUI output.
    pub fn cursor_shape(&self) -> u16 {
        match self.term.cursor_style().shape {
            alacritty_terminal::vte::ansi::CursorShape::Beam => 5,
            alacritty_terminal::vte::ansi::CursorShape::Underline => 3,
            _ => 1,
        }
    }
}

/// Find visible content only among cells Alacritty marks as occupied in this row.
fn snapshot_line_end(grid: &Grid<Cell>, line: i32) -> usize {
    let row = &grid[Line(line)];
    (0..row.occupied_len().min(grid.columns()))
        .rev()
        .find(|&col| CellView(&row[Column(col)]).has_contents())
        .map(|col| col + 1)
        .unwrap_or(0)
}

/// Emit ANSI SGR attributes without any executable shell or OSC control sequences.
fn ansi_color(out: &mut String, color: Color, background: bool) {
    let base = if background { 48 } else { 38 };
    match color {
        Color::Rgb(r, g, b) => out.push_str(&format!("\x1b[{base};2;{r};{g};{b}m")),
        Color::Idx(index) if index <= 255 => out.push_str(&format!("\x1b[{base};5;{index}m")),
        // Snapshot SGR must stay legal even when Alacritty stores a derived named color.
        Color::Idx(index @ 259..=266) => {
            out.push_str("\x1b[2m");
            let base = if background { 40 } else { 30 };
            out.push_str(&format!("\x1b[{}m", base + index - 259));
        }
        Color::Idx(267) => out.push_str("\x1b[1m"),
        Color::Idx(268) => out.push_str("\x1b[2m"),
        Color::Idx(_) => {}
        Color::Default => {}
    }
}
