//! Native screen ownership and restored-history reflow keep ConPTY redraws out of older output.

use super::*;

impl Engine {
    /// Reuse only a recognized empty prompt; pending input/custom prompts remain historical data.
    pub fn begin_process(&mut self, windows: bool, prompt: Option<&str>) {
        // A previous child may have exited in the middle of OSC/CSI or a UTF-8 sequence. New bytes
        // belong to a new stream and must not complete (or be swallowed by) that parser state.
        self.parser = Processor::new();
        self.metadata_parser = alacritty_terminal::vte::Parser::new();
        self.metadata = Default::default();
        let grid = self.term.grid();
        let end = grid.cursor.point.line;
        let start = self.logical_prompt_start();
        let mut text = String::new();
        for row in start.0..=end.0 {
            for column in 0..grid.columns() {
                let cell = &grid[Line(row)][Column(column)];
                // Wide-character spacers are layout cells, not text in the recognized prompt.
                if !cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    text.push(cell.c);
                    if let Some(combining) = cell.zerowidth() {
                        text.extend(combining);
                    }
                }
            }
        }
        self.bootstrap = (windows
            && prompt.is_some_and(|prompt| text.trim_end() == prompt.trim_end()))
        .then(Vec::new);
        self.bootstrap_line_start = false;
        self.conpty_screen = windows;
        if windows && self.bootstrap.is_none() {
            self.archive_screen();
        }
    }

    /// A newly owned PTY starts at row one, while prior step output remains ordinary scrollback.
    /// Archive only printed cells, excluding trailing viewport padding; no bytes or extra newline
    /// are injected into either the user transcript or the new program's input.
    fn archive_screen(&mut self) {
        let mut saved = self.snapshot();
        let history = saved.lines.len() - saved.rows;
        let used = saved
            .lines
            .iter()
            .skip(history)
            .rposition(|row| row.iter().any(Self::has_content))
            .map_or(0, |row| row + 1);
        saved.lines.truncate(history + used);
        while saved.lines.len() > self.history_limit {
            saved.lines.pop_front();
        }
        let prefix = saved.lines.len();
        self.term.reset_state();
        let grid = self.term.grid_mut();
        grid.scroll_up::<Color>(&(Line(0)..Line(saved.rows as i32)), prefix);
        for (row, cells) in saved.lines.into_iter().enumerate() {
            for (column, cell) in cells.into_iter().enumerate() {
                grid[Point::new(Line(row as i32 - prefix as i32), Column(column))] = cell;
            }
        }
        grid.scroll_display(Scroll::Delta((saved.offset + used).min(prefix) as i32));
    }

    /// The new console has never seen restored cells. Put the prefix in scrollback and inherit
    /// row one, so its later absolute redraw cannot erase that prefix. Keeping the display offset
    /// preserves the initial restored view; ordinary input returns to the live bottom as usual.
    pub(super) fn start_restored_screen(&mut self) -> Line {
        let start = self.visible_prompt_start();
        if start.0 > 0 {
            let offset = self.offset() + start.0 as usize;
            let rows = self.term.screen_lines();
            let grid = self.term.grid_mut();
            grid.scroll_up::<Color>(&(Line(0)..Line(rows as i32)), start.0 as usize);
            grid.cursor.point.line -= start.0;
            let offset = offset.min(self.history());
            let current = self.offset();
            self.term
                .scroll_display(Scroll::Delta(offset as i32 - current as i32));
        }
        Line(0)
    }

    /// Fill unused viewport rows with retained history while keeping the current native screen and
    /// caret visible. This changes only presentation offset, never the coordinates ConPTY redraws.
    pub(super) fn reveal_native_screen(&mut self) {
        if !self.conpty_screen
            || !self.follow_screen
            || self.term.mode().contains(TermMode::ALT_SCREEN)
        {
            return;
        }
        let grid = self.term.grid();
        let used = (0..grid.screen_lines())
            .rfind(|&row| {
                (0..grid.columns())
                    .any(|column| Self::has_content(&grid[Line(row as i32)][Column(column)]))
            })
            .map_or(0, |row| row + 1);
        let used = used.max(grid.cursor.point.line.0 as usize + 1);
        let offset = grid.history_size().min(grid.screen_lines() - used);
        self.term
            .scroll_display(Scroll::Delta(offset as i32 - self.offset() as i32));
    }
    /// Follow only soft wraps; hard line breaks belong to historical output and are never removed.
    fn logical_prompt_start(&self) -> Line {
        let grid = self.term.grid();
        let mut start = grid.cursor.point.line;
        while start.0 > -(grid.history_size() as i32)
            && grid[Line(start.0 - 1)][Column(grid.columns() - 1)]
                .flags
                .contains(Flags::WRAPLINE)
        {
            start.0 -= 1;
        }
        start
    }
    /// A prompt taller than the viewport cannot inherit a negative ConPTY screen position.
    /// Remove only that recognized empty prompt and retain every preceding logical history row;
    /// the new Shell then redraws the same prompt once from the first visible row.
    pub(super) fn visible_prompt_start(&mut self) -> Line {
        let start = self.logical_prompt_start();
        if start.0 >= 0 {
            return start;
        }
        let mut saved = self.snapshot();
        let prefix = saved.lines.len() - saved.rows;
        let prefix = (prefix as i32 + start.0) as usize;
        saved.lines.truncate(prefix);
        let offset = saved.offset.min(prefix);
        self.term.reset_state();
        let grid = self.term.grid_mut();
        grid.scroll_up::<Color>(&(Line(0)..Line(saved.rows as i32)), prefix);
        for (row, cells) in saved.lines.into_iter().enumerate() {
            for (column, cell) in cells.into_iter().enumerate() {
                grid[Point::new(Line(row as i32 - prefix as i32), Column(column))] = cell;
            }
        }
        grid.scroll_display(Scroll::Delta(offset as i32));
        Line(0)
    }

    /// ConPTY redraws only its own screen, unlike a POSIX shell. Temporarily detach scrollback
    /// while Alacritty resizes the live screen, then reattach reflowed history. Otherwise height
    /// growth pulls imported history into blank native rows and the next redraw destroys it.
    pub(super) fn resize_conpty_screen(&mut self, size: GridSize) {
        // Most pixel resize events keep the same character geometry. Check that cheaply before
        // cloning bounded but potentially large history on the UI thread.
        if self.term.screen_lines() == size.rows && self.term.columns() == size.columns {
            return;
        }
        let saved = self.snapshot();
        let old_offset = saved.offset;
        let history = saved.lines.len() - saved.rows;
        let mut prefix = saved.lines;
        prefix.truncate(history);
        self.term.clear_screen(ClearMode::Saved);
        // The native console owns screen reflow and sends an absolute redraw. Reflowing those
        // rows a second time can push a partial prompt into history before that redraw arrives.
        // Resize Term normally for its tabs/alternate buffer/margins, but keep the live grid's
        // coordinates until ConPTY supplies its authoritative new screen.
        let mut native_grid = self.term.grid().clone();
        native_grid.resize::<Color>(false, size.rows, size.columns);
        self.term.resize(size);
        *self.term.grid_mut() = native_grid;
        if prefix.is_empty() {
            return;
        }
        // Alacritty remains responsible for wide glyphs and wrap flags in detached history too.
        let mut history_engine = Engine::new(
            GridSize {
                columns: saved.columns,
                rows: 1,
            },
            self.history_limit,
        );
        prefix.push_back(Vec::new());
        history_engine
            .restore(SavedGrid {
                columns: saved.columns,
                rows: 1,
                lines: prefix,
                cursor: (0, 0),
                input_needs_wrap: false,
                offset: 0,
            })
            .expect("validated in-memory history");
        history_engine.resize(GridSize {
            columns: size.columns,
            rows: 1,
        });
        let mut prefix = history_engine.snapshot().lines;
        prefix.pop_back();
        let mut resized = self.snapshot();
        prefix.append(&mut resized.lines);
        while prefix.len() > self.history_limit + size.rows {
            prefix.pop_front();
        }
        resized.lines = prefix;
        resized.offset = old_offset.min(resized.lines.len() - size.rows);
        self.term.clear_screen(ClearMode::Saved);
        self.restore(resized)
            .expect("validated resized in-memory grid");
    }
}
