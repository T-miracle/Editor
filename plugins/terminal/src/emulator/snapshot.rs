//! Persist styled primary output without executable shell input or live process modes.
use super::*;
use std::collections::VecDeque;

impl Emulator {
    /// Legacy physical rows are joined using saved wrap flags before replaying into the new core.
    pub fn restore(&mut self, output: &str, display: Option<&DisplayState>) {
        if let Some(display) = display {
            let output = if display.soft_wraps {
                output.to_owned()
            } else {
                let lines: Vec<_> = output.split("\r\n").collect();
                let start = i32::from(display.rows) - lines.len() as i32;
                let mut joined = String::new();
                for (index, line) in lines.iter().enumerate() {
                    joined.push_str(line);
                    if index + 1 < lines.len()
                        && !display.wrapped_lines.contains(&(start + index as i32))
                    {
                        joined.push_str("\r\n");
                    }
                }
                joined
            };
            self.parser.process(output.as_bytes());
            if let Some(cursor) = &display.cursor_bytes {
                self.parser.process(cursor.as_bytes());
            } else {
                let (rows, cols) = self.screen().size();
                let (row, col) = (
                    display.cursor.0.min(rows - 1),
                    display.cursor.1.min(cols - 1),
                );
                self.parser
                    .process(format!("\x1b[{};{}H", row + 1, col + 1).as_bytes());
                if display.wrap_pending {
                    // Reprinting the final cell restores pending-wrap state through public VT input.
                    if let Some(cell) = self.parser.screen().cell(row, col) {
                        let value = styled_cell(cell);
                        self.parser.process(value.as_bytes());
                    }
                }
            }
            self.set_scrollback(display.scrollback);
        } else {
            let marker = "\x1b[0m\x1b[0m--- restored session; new shell ---\x1b[0m\r\n";
            let output = output
                .replace(&format!("\x1b[0m\r\n{marker}"), "")
                .replace(marker, "");
            self.parser
                .process(output.strip_suffix("\r\n").unwrap_or(&output).as_bytes());
        }
        self.replies.borrow_mut().bytes.clear();
        self.refresh_history();
    }
    /// Only compact an old generated gap bounded by identical empty default prompts.
    pub fn repair_legacy_prompt_gap(&mut self, prompt: &str) {
        let screen = self.parser.screen();
        let (row, col) = screen.cursor_position();
        if row <= 1
            || line_text(screen, row).trim_end() != prompt.trim_end()
            || screen.row_wrapped(row)
            || (row + 1..screen.size().0).any(|r| line_end(screen, r) > 0)
        {
            return;
        }
        let Some(previous) = (0..row).rev().find(|&r| line_end(screen, r) > 0) else {
            return;
        };
        let gap = row - previous - 1;
        if gap == 0
            || line_text(screen, previous).trim_end() != prompt.trim_end()
            || screen.row_wrapped(previous)
        {
            return;
        }
        self.parser.process(
            format!(
                "\x1b[{};1H\x1b[{gap}M\x1b[{};{}H",
                previous + 2,
                row - gap + 1,
                col + 1
            )
            .as_bytes(),
        );
    }
    #[cfg(test)]
    pub fn snapshot(&self, budget: usize) -> String {
        self.snapshot_with_display(budget).0
    }
    /// Snapshot size is bounded from newest rows backward; full-screen TUIs never replace shell history.
    pub fn snapshot_with_display(&self, budget: usize) -> (String, DisplayState) {
        let primary = primary_screen(self.parser.screen());
        let (rows, columns) = primary.size();
        let cursor = primary.cursor_position();
        let (output, wrapped_lines) = transcript(&primary, history_size(&primary), budget);
        (
            output,
            DisplayState {
                rows,
                columns,
                cursor: (cursor.0, cursor.1.min(columns - 1)),
                wrap_pending: cursor.1 >= columns,
                scrollback: primary.scrollback(),
                wrapped_lines,
                cursor_bytes: Some(
                    String::from_utf8_lossy(&primary.cursor_state_formatted()).into(),
                ),
                soft_wraps: true,
            },
        )
    }
}

/// Build physical rows through public cells; soft wraps replay without an artificial hard newline.
pub(super) fn transcript(
    screen: &vt100::Screen,
    history: usize,
    budget: usize,
) -> (String, Vec<i32>) {
    let mut view = screen.clone();
    view.set_scrollback(0);
    let (rows, _) = view.size();
    if history == 0
        && view.cursor_position() == (0, 0)
        && (0..rows).all(|r| line_end(&view, r) == 0)
    {
        return (String::new(), Vec::new());
    }
    let mut lines = VecDeque::new();
    let mut wrapped = Vec::new();
    let mut bytes = 0;
    for line in (-(history as i32)..i32::from(rows)).rev() {
        view.set_scrollback((-line).max(0) as usize);
        let row = line.max(0) as u16;
        let mut value = String::from("\x1b[0m");
        let mut previous = None;
        for col in 0..line_end(&view, row) {
            if let Some(cell) = view
                .cell(row, col)
                .filter(|cell| !cell.is_wide_continuation())
            {
                let style = (
                    cell.fgcolor(),
                    cell.bgcolor(),
                    cell.bold(),
                    cell.dim(),
                    cell.italic(),
                    cell.underline(),
                    cell.inverse(),
                );
                if previous == Some(style) {
                    value.push_str(if cell.contents().is_empty() {
                        " "
                    } else {
                        cell.contents()
                    });
                } else {
                    value.push_str(&styled_cell(cell));
                    previous = Some(style);
                }
            }
        }
        value.push_str("\x1b[0m");
        if line < i32::from(rows) - 1 && !view.row_wrapped(row) {
            value.push_str("\r\n");
        }
        if bytes + value.len() > budget {
            break;
        }
        bytes += value.len();
        lines.push_front(value);
        if view.row_wrapped(row) {
            wrapped.push(line);
        }
    }
    (lines.into_iter().collect(), wrapped)
}
/// Styled blank cells are content even when ordinary text selection ignores them.
fn line_end(screen: &vt100::Screen, row: u16) -> u16 {
    (0..screen.size().1)
        .rev()
        .find(|&col| {
            screen.cell(row, col).is_some_and(|c| {
                c.has_contents()
                    || c.is_wide_continuation()
                    || c.bgcolor() != vt100::Color::Default
                    || c.inverse()
                    || c.underline()
            })
        })
        .map(|col| col + 1)
        .unwrap_or(0)
}
/// Only SGR styling and glyphs are serialized; OSC queries and shell commands are excluded.
fn styled_cell(cell: &vt100::Cell) -> String {
    let mut value = String::from("\x1b[0m");
    for (color, base) in [(cell.fgcolor(), 38), (cell.bgcolor(), 48)] {
        match color {
            vt100::Color::Idx(index) => value.push_str(&format!("\x1b[{base};5;{index}m")),
            vt100::Color::Rgb(r, g, b) => value.push_str(&format!("\x1b[{base};2;{r};{g};{b}m")),
            vt100::Color::Default => {}
        }
    }
    for (enabled, code) in [
        (cell.bold(), 1),
        (cell.dim(), 2),
        (cell.italic(), 3),
        (cell.underline(), 4),
        (cell.inverse(), 7),
    ] {
        if enabled {
            value.push_str(&format!("\x1b[{code}m"));
        }
    }
    value.push_str(if cell.contents().is_empty() {
        " "
    } else {
        cell.contents()
    });
    value
}
