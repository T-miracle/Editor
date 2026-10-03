//! Local text selection uses stable history-relative coordinates, independent of native input.
use super::*;
impl Emulator {
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
}
