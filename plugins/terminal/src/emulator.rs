//! Upstream VT parsing and reflow run entirely in the WASM guest.
use serde::{Deserialize, Serialize};
use std::cell::{RefCell, RefMut};
use std::rc::Rc;
mod replies;
mod selection;
mod snapshot;

/// Palette-independent colors shared by painting and saved transcripts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Color {
    Default,
    Idx(u16),
    Rgb(u8, u8, u8),
}
fn color(value: vt100::Color) -> Color {
    match value {
        vt100::Color::Default => Color::Default,
        vt100::Color::Idx(i) => Color::Idx(i.into()),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}
/// Guest-owned responses and dynamic appearance; no native authority enters the parser.
#[derive(Default)]
pub(super) struct Replies {
    pub bytes: Vec<u8>,
    pub palette: Vec<u32>,
    pub colors: std::collections::BTreeMap<usize, u32>,
    pub cell_size: (u16, u16),
    pub grid_size: (u16, u16),
    focus: bool,
    cursor: u16,
}
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

/// Rendering borrows the live viewport. Historical selection lazily clones once, never per cell.
pub(super) struct Screen<'a> {
    view: &'a vt100::Screen,
    history: RefCell<Option<vt100::Screen>>,
}
impl Screen<'_> {
    pub fn size(&self) -> (u16, u16) {
        self.view.size()
    }
    pub fn scrollback(&self) -> usize {
        self.view.scrollback()
    }
    pub fn alternate_screen(&self) -> bool {
        self.view.alternate_screen()
    }
    pub fn application_cursor(&self) -> bool {
        self.view.application_cursor()
    }
    pub fn bracketed_paste(&self) -> bool {
        self.view.bracketed_paste()
    }
    pub fn hide_cursor(&self) -> bool {
        self.view.hide_cursor()
    }
    pub fn cursor_position(&self) -> (u16, u16) {
        let (r, c) = self.view.cursor_position();
        (r, c.min(self.size().1 - 1))
    }
    pub fn mouse_protocol_mode(&self) -> MouseProtocolMode {
        match self.view.mouse_protocol_mode() {
            vt100::MouseProtocolMode::None => MouseProtocolMode::None,
            vt100::MouseProtocolMode::Press | vt100::MouseProtocolMode::PressRelease => {
                MouseProtocolMode::Press
            }
            vt100::MouseProtocolMode::ButtonMotion => MouseProtocolMode::ButtonMotion,
            vt100::MouseProtocolMode::AnyMotion => MouseProtocolMode::AnyMotion,
        }
    }
    pub fn mouse_protocol_encoding(&self) -> MouseProtocolEncoding {
        match self.view.mouse_protocol_encoding() {
            vt100::MouseProtocolEncoding::Sgr => MouseProtocolEncoding::Sgr,
            _ => MouseProtocolEncoding::Legacy,
        }
    }
    pub fn cell(&self, row: u16, col: u16) -> Option<CellView> {
        self.view.cell(row, col).cloned().map(CellView)
    }
    /// Padding is excluded; spaces between printed glyphs remain selectable.
    pub fn content_end(&self, row: u16) -> u16 {
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
    pub fn cell_line(&self, line: i32, col: u16) -> Option<CellView> {
        self.with_line(line, |screen, row| {
            screen.cell(row, col).cloned().map(CellView)
        })
        .flatten()
    }
    pub fn row_wrapped(&self, line: i32) -> bool {
        self.with_line(line, |screen, row| screen.row_wrapped(row))
            .unwrap_or(false)
    }
    /// A separate historical viewport avoids mutating the screen displayed to the user.
    fn with_line<T>(&self, line: i32, read: impl FnOnce(&vt100::Screen, u16) -> T) -> Option<T> {
        let row = line + self.scrollback() as i32;
        if row >= 0 && row < self.size().0 as i32 {
            return Some(read(self.view, row as u16));
        }
        if line >= self.size().0 as i32 {
            return None;
        }
        let mut history = self.history.borrow_mut();
        let history = history.get_or_insert_with(|| self.view.clone());
        let offset = (-line).max(0) as usize;
        history.set_scrollback(offset);
        if history.scrollback() != offset {
            return None;
        }
        Some(read(history, line.max(0) as u16))
    }
    #[cfg(test)]
    pub fn contents(&self) -> String {
        self.view
            .rows(0, self.size().1)
            .collect::<Vec<_>>()
            .join("\n")
    }
}
/// Owned cell views let historical lookup release its temporary viewport borrow.
pub(super) struct CellView(vt100::Cell);
impl CellView {
    pub fn contents(&self) -> String {
        if self.0.contents().is_empty() {
            " ".into()
        } else {
            self.0.contents().into()
        }
    }
    pub fn has_contents(&self) -> bool {
        !self.0.contents().trim_end_matches(' ').is_empty()
    }
    pub fn is_wide_continuation(&self) -> bool {
        self.0.is_wide_continuation()
    }
    pub fn is_wide(&self) -> bool {
        self.0.is_wide()
    }
    pub fn fgcolor(&self) -> Color {
        match color(self.0.fgcolor()) {
            Color::Default if self.0.dim() => Color::Idx(268),
            Color::Default if self.0.bold() => Color::Idx(267),
            Color::Idx(i @ 0..=7) if self.0.dim() => Color::Idx(i + 259),
            Color::Idx(i @ 0..=7) if self.0.bold() => Color::Idx(i + 8),
            value => value,
        }
    }
    pub fn bgcolor(&self) -> Color {
        color(self.0.bgcolor())
    }
    pub fn bold(&self) -> bool {
        self.0.bold()
    }
    pub fn underline(&self) -> bool {
        self.0.underline()
    }
    pub fn inverse(&self) -> bool {
        self.0.inverse()
    }
}
/// Selection anchors remain stable as the user scrolls the viewport.
#[derive(Clone, Copy)]
struct Selection {
    anchor: (i32, u16),
    end: (i32, u16),
    clicks: u8,
}
/// Schema-one metadata remains readable; new payloads carry precise caret state and soft wraps.
#[derive(Serialize, Deserialize)]
pub(super) struct DisplayState {
    rows: u16,
    columns: u16,
    cursor: (u16, u16),
    wrap_pending: bool,
    scrollback: usize,
    wrapped_lines: Vec<i32>,
    #[serde(default)]
    cursor_bytes: Option<String>,
    #[serde(default)]
    soft_wraps: bool,
}
impl DisplayState {
    pub fn size(&self) -> (u16, u16) {
        (self.rows.clamp(1, 500), self.columns.clamp(2, 1000))
    }
}
/// Upstream owns both screen buffers; the guest owns selection and saved presentation state.
pub(super) struct Emulator {
    parser: vt100::Parser<replies::Listener>,
    replies: Rc<RefCell<Replies>>,
    limit: usize,
    /// Cached after mutations so painting never clones an entire scrollback buffer.
    history_len: usize,
    selection: Option<Selection>,
    bootstrap: Option<Vec<u8>>,
    bootstrap_line_start: bool,
}
impl Emulator {
    pub fn new(rows: u16, cols: u16, limit: usize) -> Self {
        let replies = Rc::new(RefCell::new(Replies {
            palette: vec![0; 269],
            cell_size: (8, 21),
            grid_size: (rows, cols),
            cursor: 5,
            ..Replies::default()
        }));
        Self {
            parser: vt100::Parser::new_with_callbacks(
                rows.max(1),
                cols.max(2),
                limit,
                replies::Listener(replies.clone()),
            ),
            replies,
            limit,
            history_len: 0,
            selection: None,
            bootstrap: None,
            bootstrap_line_start: false,
        }
    }
    pub fn screen(&self) -> Screen<'_> {
        Screen {
            view: self.parser.screen(),
            history: RefCell::new(None),
        }
    }
    pub fn replies_mut(&self) -> RefMut<'_, Replies> {
        self.replies.borrow_mut()
    }
    /// ConPTY may reuse an empty saved prompt, never pending input.
    pub fn begin_process(&mut self, windows: bool, prompt: Option<&str>) {
        let screen = self.parser.screen();
        self.bootstrap = (windows
            && prompt.is_some_and(|prompt| {
                line_text(screen, screen.cursor_position().0).trim_end() == prompt.trim_end()
            }))
        .then(Vec::new);
        self.bootstrap_line_start = false;
    }
    /// Only the first ConPTY query is special; ordinary output stays in the upstream parser.
    pub fn process(&mut self, bytes: &[u8]) {
        const QUERY: &[u8] = b"\x1b[6n";
        let buffered = if let Some(mut initial) = self.bootstrap.take() {
            initial.extend_from_slice(bytes);
            if initial.len() < QUERY.len() && QUERY.starts_with(&initial) {
                self.bootstrap = Some(initial);
                return;
            }
            if initial.starts_with(QUERY) {
                let row = self.screen().cursor_position().0 + 1;
                self.replies
                    .borrow_mut()
                    .bytes
                    .extend_from_slice(format!("\x1b[{row};1R").as_bytes());
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
            self.parser.process(b"\r");
            self.bootstrap_line_start = false;
        }
        self.parser.process(bytes);
        self.refresh_history();
        self.selection = None;
    }
    /// Reflow primary lines upstream; full-screen programs keep absolute coordinates.
    pub fn resize(&mut self, rows: u16, cols: u16) {
        self.parser.screen_mut().set_size(rows.max(1), cols.max(2));
        self.refresh_history();
        self.replies.borrow_mut().grid_size = (rows, cols);
        self.selection = None;
    }
    pub fn history(&self) -> usize {
        if self.screen().alternate_screen() {
            return 0;
        }
        self.history_len
    }
    pub fn scroll(&mut self, delta: i32) {
        self.set_scrollback((self.screen().scrollback() as i64 + i64::from(delta)).max(0) as usize);
    }
    pub fn set_scrollback(&mut self, offset: usize) {
        self.parser.screen_mut().set_scrollback(offset);
    }
    /// Replace screen data while retaining split escape sequences in the live parser.
    pub fn set_history_limit(&mut self, limit: usize) {
        if self.limit == limit {
            return;
        }
        self.replace_history(limit, false);
        self.limit = limit;
    }
    pub fn clear_history(&mut self) {
        self.replace_history(self.limit, true);
        self.selection = None;
    }
    /// Clearing both buffers preserves interactive input modes and process identity.
    pub fn clear_buffer(&mut self) {
        let (rows, cols) = self.screen().size();
        let alternate = self.screen().alternate_screen();
        let modes = self.parser.screen().input_mode_formatted();
        let mut blank = vt100::Parser::new(rows, cols, self.limit);
        blank.process(&modes);
        if alternate {
            blank.process(b"\x1b[?1049h");
        }
        *self.parser.screen_mut() = blank.screen().clone();
        self.selection = None;
        self.refresh_history();
    }
    /// Temporary parsing changes history capacity without resetting live parsing or alternate output.
    fn replace_history(&mut self, limit: usize, clear: bool) {
        let primary = primary_screen(self.parser.screen());
        let (rows, cols) = primary.size();
        let history = if clear {
            0
        } else {
            history_size(&primary).min(limit)
        };
        let output = snapshot::transcript(&primary, history, usize::MAX).0;
        let mut replacement = vt100::Parser::new(rows, cols, limit);
        replacement.process(output.as_bytes());
        replacement.process(&primary.cursor_state_formatted());
        replacement.process(&primary.input_mode_formatted());
        replacement.process(&primary.attributes_formatted());
        if self.screen().alternate_screen() {
            replacement.process(b"\x1b[?1049h");
            replacement.process(&self.parser.screen().state_formatted());
        }
        replacement
            .screen_mut()
            .set_scrollback(primary.scrollback().min(history));
        *self.parser.screen_mut() = replacement.screen().clone();
        self.refresh_history();
    }
    /// Offset clamping is constant-time and restores the exact current viewport immediately.
    fn refresh_history(&mut self) {
        let screen = self.parser.screen_mut();
        let offset = screen.scrollback();
        screen.set_scrollback(usize::MAX);
        self.history_len = screen.scrollback();
        screen.set_scrollback(offset);
    }
    pub fn color_override(&self, index: usize) -> Option<u32> {
        self.replies.borrow().colors.get(&index).copied()
    }
    pub fn focus_mode(&self) -> bool {
        self.replies.borrow().focus
    }
    pub fn cursor_shape(&self) -> u16 {
        self.replies.borrow().cursor
    }
}
/// Inspect primary state on an isolated parser; live full-screen applications remain untouched.
fn primary_screen(screen: &vt100::Screen) -> vt100::Screen {
    let (rows, cols) = screen.size();
    let mut copy = vt100::Parser::new(rows, cols, 0);
    *copy.screen_mut() = screen.clone();
    if screen.alternate_screen() {
        copy.process(b"\x1b[?1049l");
    }
    copy.screen().clone()
}
/// Scroll clamping exposes the retained history size without reaching into upstream private grids.
fn history_size(screen: &vt100::Screen) -> usize {
    let mut copy = screen.clone();
    copy.set_scrollback(usize::MAX);
    copy.scrollback()
}
fn line_text(screen: &vt100::Screen, row: u16) -> String {
    screen
        .rows(0, screen.size().1)
        .nth(row as usize)
        .unwrap_or_default()
}
