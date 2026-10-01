//! The terminal guest owns Alacritty's VT parser, cells, modes and scrollback.
use alacritty_terminal::Term;
use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::{Dimensions, Grid, GridCell, Scroll};
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::{Config, TermMode};
use alacritty_terminal::vte::ansi::{
    Color as CoreColor, CursorShape, CursorStyle, NamedColor, Processor, Rgb,
};
use serde::{Deserialize, Serialize};
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

/// Presentation metadata supplements ANSI text without persisting a running shell's modes.
#[derive(Serialize, Deserialize)]
pub(super) struct DisplayState {
    rows: u16,
    columns: u16,
    cursor: (u16, u16),
    wrap_pending: bool,
    scrollback: usize,
    /// History-relative row numbers distinguish soft wraps from real line breaks.
    wrapped_lines: Vec<i32>,
}
impl DisplayState {
    /// Bound user-controlled dimensions before allocating an emulator on restore.
    pub(super) fn size(&self) -> (u16, u16) {
        (self.rows.clamp(1, 500), self.columns.clamp(2, 1000))
    }
}

/// Single terminal session: upstream parser and grid plus editor-facing presentation state.
pub(super) struct Emulator {
    term: Term<Listener>,
    parser: Processor,
    replies: Rc<RefCell<Replies>>,
    selection: Option<Selection>,
    /// Only a new Windows PTY's first output can be its inherited-cursor bootstrap query.
    bootstrap: Option<Vec<u8>>,
    /// Synchronize the parser with the inherited line origin when real process output arrives.
    bootstrap_line_start: bool,
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
            bootstrap: None,
            bootstrap_line_start: false,
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
    /// Reuse a saved empty default prompt, while keeping pending input and task output intact.
    pub fn begin_process(&mut self, windows: bool, prompt: Option<&str>) {
        let grid = self.term.grid();
        self.bootstrap = (windows
            && prompt
                .is_some_and(|prompt| grid_line_text(grid, grid.cursor.point.line.0) == prompt))
        .then(Vec::new);
        self.bootstrap_line_start = false;
    }
    /// Feed PTY output to Alacritty; no output is sent back as shell input.
    pub fn process(&mut self, bytes: &[u8]) {
        // portable-pty enables ConPTY cursor inheritance. A saved prompt's final column makes
        // PowerShell insert a newline. Startup can look correct until the first height redraw
        // clears the saved prompt row and exposes the gap in the restored command transcript.
        // Answer only this initial query with column one; normal application DSR stays upstream.
        const INHERIT_QUERY: &[u8] = b"\x1b[6n";
        let buffered = if let Some(mut initial) = self.bootstrap.take() {
            initial.extend_from_slice(bytes);
            if initial.len() < INHERIT_QUERY.len() && INHERIT_QUERY.starts_with(&initial) {
                self.bootstrap = Some(initial);
                return;
            }
            if initial.starts_with(INHERIT_QUERY) {
                let row = self.screen().cursor_position().0 + 1;
                self.replies
                    .borrow_mut()
                    .bytes
                    .extend_from_slice(format!("\x1b[{row};1R").as_bytes());
                self.bootstrap_line_start = true;
                Some(initial.split_off(INHERIT_QUERY.len()))
            } else {
                // Shells without this bootstrap keep all original bytes, including split ESCs.
                Some(initial)
            }
        } else {
            None
        };
        let bytes = buffered.as_deref().unwrap_or(bytes);
        if self.bootstrap_line_start && !bytes.is_empty() {
            // ConPTY may emit the prompt without CUP when no resize redraw is needed. Match
            // its reported column before parsing output, but keep the saved caret until then.
            let grid = self.term.grid_mut();
            grid.cursor.point.column = Column(0);
            grid.cursor.input_needs_wrap = false;
            self.bootstrap_line_start = false;
        }
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
    /// Replay display data locally and restore its caret without inserting a separator or newline.
    pub fn restore(&mut self, output: &str, display: Option<&DisplayState>) {
        if let Some(display) = display {
            self.process(output.as_bytes());
            let grid = self.term.primary_grid_mut();
            let rows = grid.screen_lines();
            let columns = grid.columns();
            grid.cursor.point = Point::new(
                Line(i32::from(display.cursor.0).min(rows as i32 - 1)),
                Column(usize::from(display.cursor.1).min(columns - 1)),
            );
            grid.cursor.input_needs_wrap = display.wrap_pending;
            // ANSI replay uses hard breaks for exact rows; restore their original reflow flags.
            for &line in &display.wrapped_lines {
                if line >= -(grid.history_size() as i32) && line < rows as i32 {
                    grid[Point::new(Line(line), Column(columns - 1))]
                        .flags
                        .insert(Flags::WRAPLINE);
                }
            }
            self.set_scrollback(display.scrollback.min(self.history()));
        } else {
            // Legacy snapshots added a writer newline and a generated restoration separator.
            // Remove only that exact legacy row, leaving user output and internal breaks intact.
            let marker = "\x1b[0m\x1b[0m--- restored session; new shell ---\x1b[0m\r\n";
            let output = output
                .replace(&format!("\x1b[0m\r\n{marker}"), "")
                .replace(marker, "");
            self.process(output.strip_suffix("\r\n").unwrap_or(&output).as_bytes());
        }
        // A snapshot is never allowed to issue terminal query responses to the new process.
        self.replies.borrow_mut().bytes.clear();
    }
    /// Migrate the old startup artifact only between two identical empty default prompts.
    pub fn repair_legacy_prompt_gap(&mut self, prompt: &str) {
        let grid = self.term.primary_grid();
        let cursor = grid.cursor.clone();
        let row = cursor.point.line.0;
        // Never compact printed commands, colored blank output, wrapped prompts or later content.
        if row <= 1
            || grid_line_text(grid, row) != prompt
            || grid[Line(row)][Column(grid.columns() - 1)]
                .flags
                .contains(Flags::WRAPLINE)
            || (row + 1..grid.screen_lines() as i32).any(|line| snapshot_line_end(grid, line) > 0)
        {
            return;
        }
        let Some(previous) = (0..row)
            .rev()
            .find(|&line| snapshot_line_end(grid, line) > 0)
        else {
            return;
        };
        let gap = row - previous - 1;
        if gap == 0
            || grid_line_text(grid, previous) != prompt
            || grid[Line(previous)][Column(grid.columns() - 1)]
                .flags
                .contains(Flags::WRAPLINE)
        {
            return;
        }
        let offset = grid.display_offset();
        self.process(format!("\x1b[{};1H\x1b[{gap}M", previous + 2).as_bytes());
        let grid = self.term.primary_grid_mut();
        grid.cursor = cursor;
        grid.cursor.point.line -= gap;
        self.set_scrollback(offset.min(self.history()));
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
    /// Erase current and primary buffers while retaining the live process and its input modes.
    pub fn clear_buffer(&mut self) {
        self.term.grid_mut().reset::<CoreColor>();
        if self.screen().alternate_screen() {
            self.term.primary_grid_mut().reset::<CoreColor>();
        }
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
    /// Capture the primary viewport even when a TUI temporarily owns the alternate screen.
    fn display_state(&self, wrapped_lines: Vec<i32>) -> DisplayState {
        let grid = self.term.primary_grid();
        let rows = grid.screen_lines();
        let columns = grid.columns();
        DisplayState {
            rows: rows as u16,
            columns: columns as u16,
            cursor: (
                grid.cursor.point.line.0.max(0) as u16,
                grid.cursor.point.column.0 as u16,
            ),
            wrap_pending: grid.cursor.input_needs_wrap,
            scrollback: grid.display_offset(),
            wrapped_lines,
        }
    }
    /// Tests can inspect the ANSI payload without unpacking its presentation metadata.
    #[cfg(test)]
    pub fn snapshot(&self, budget: usize) -> String {
        self.snapshot_with_display(budget).0
    }
    /// Save only styled primary-screen output, compatible with the existing ANSI snapshot schema.
    pub fn snapshot_with_display(&self, budget: usize) -> (String, DisplayState) {
        let grid = self.term.primary_grid();
        let rows = grid.screen_lines();
        if grid.history_size() == 0
            && grid.cursor.point == Point::new(Line(0), Column(0))
            && (0..rows).all(|row| snapshot_line_end(grid, row as i32) == 0)
        {
            return (String::new(), self.display_state(Vec::new()));
        }
        let mut lines = VecDeque::new();
        let mut bytes = 0;
        let mut wrapped_lines = Vec::new();
        // Traverse newest lines first and stop as soon as the storage budget is full.
        // Existing blank screen rows keep history in the same viewport; no extra row is appended.
        for line in (-(grid.history_size() as i32)..rows as i32).rev() {
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
            value.push_str("\x1b[0m");
            if line < rows as i32 - 1 {
                value.push_str("\r\n");
            }
            if value.len() > budget {
                continue;
            }
            if bytes + value.len() > budget {
                break;
            }
            bytes += value.len();
            lines.push_front(value);
            // Capture flags only for retained lines so metadata obeys the same storage/fuel cap.
            if row[Column(grid.columns() - 1)]
                .flags
                .contains(Flags::WRAPLINE)
            {
                wrapped_lines.push(line);
            }
        }
        (
            lines.into_iter().collect(),
            self.display_state(wrapped_lines),
        )
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

/// Preserve styled trailing spaces as well as printed characters in occupied cells.
fn snapshot_line_end(grid: &Grid<Cell>, line: i32) -> usize {
    let row = &grid[Line(line)];
    (0..row.occupied_len().min(grid.columns()))
        .rev()
        .find(|&col| !row[Column(col)].is_empty())
        .map(|col| col + 1)
        .unwrap_or(0)
}

/// Read one physical row for prompt matching, including combining marks and excluding padding.
fn grid_line_text(grid: &Grid<Cell>, line: i32) -> String {
    let mut text = String::new();
    for col in 0..grid[Line(line)].occupied_len().min(grid.columns()) {
        let cell = &grid[Point::new(Line(line), Column(col))];
        if !cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
            text.push(cell.c);
            if let Some(extra) = cell.zerowidth() {
                text.extend(extra.iter());
            }
        }
    }
    text.trim_end().to_owned()
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
