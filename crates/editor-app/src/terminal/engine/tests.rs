//! Parser-boundary regressions supplement the real Shell/TUI editor integration tests.

use super::*;

/// Native mouse coordinates exclude the retained prefix above the new screen.
#[test]
fn native_mouse_coordinates_ignore_visible_history_rows() {
    let mut engine = Engine::new(
        GridSize {
            columns: 80,
            rows: 12,
        },
        100,
    );
    engine.process(b"OLD_ONE\r\nOLD_TWO\r\n");
    engine.begin_process(true, None);
    engine.process(b"BTN\x1b[?1000h\x1b[?1006h");
    assert_eq!(engine.offset(), 2);
    assert_eq!(engine.mouse_position(2, 0), Some((0, 0)));
    assert_eq!(engine.mouse_position(0, 0), None);
}

/// Painted trailing spaces survive a child boundary just like visible letters.
#[test]
fn styled_trailing_spaces_are_retained_at_a_new_native_screen() {
    let mut engine = Engine::new(
        GridSize {
            columns: 80,
            rows: 12,
        },
        100,
    );
    engine.process(b"OUTPUT\r\n\x1b[41m   ");
    engine.begin_process(true, None);
    let grid = engine.snapshot();
    assert_eq!(grid.lines[1].len(), 3);
    assert!(
        grid.lines[1]
            .iter()
            .all(|cell| cell.c == ' ' && cell.bg == Color::Named(NamedColor::Red))
    );
}

/// A new child must not complete an unterminated escape sequence from an earlier task step.
#[test]
fn a_new_native_stream_does_not_inherit_an_incomplete_osc() {
    let mut engine = Engine::new(
        GridSize {
            columns: 80,
            rows: 12,
        },
        100,
    );
    engine.process(b"\x1b]0;unfinished title");
    engine.begin_process(true, None);
    engine.process(b"NEXT_STEP_RESULT");
    let grid = engine.snapshot();
    let text: String = grid.lines.iter().flatten().map(|cell| cell.c).collect();
    assert!(text.contains("NEXT_STEP_RESULT"), "{text}");
}

/// A finished native program cannot redraw clipped glyphs; its retained grid must reflow itself.
#[test]
fn ended_native_output_survives_narrow_then_wide_resize() {
    let mut engine = Engine::new(
        GridSize {
            columns: 80,
            rows: 12,
        },
        100,
    );
    engine.begin_process(true, None);
    engine.process(format!("{}RIGHT_HALF_MARKER", "x".repeat(50)).as_bytes());
    engine.end_process();
    engine.resize(GridSize {
        columns: 40,
        rows: 8,
    });
    engine.resize(GridSize {
        columns: 80,
        rows: 12,
    });
    let grid = engine.snapshot();
    let text: String = grid.lines.iter().flatten().map(|cell| cell.c).collect();
    assert_eq!(text.matches("RIGHT_HALF_MARKER").count(), 1, "{text}");
}

/// Absolute native redraws belong to the new child, not to output from the previous task step.
#[test]
fn a_new_native_screen_cannot_clear_prior_step_output() {
    let mut engine = Engine::new(
        GridSize {
            columns: 40,
            rows: 12,
        },
        100,
    );
    engine.process(b"BUILD_HEADER\r\nBUILD_ONLY\r\n");
    engine.begin_process(true, None);
    engine.process(b"\x1b[H\x1b[JPRELAUNCH_ONLY\r\n");
    engine.resize(GridSize {
        columns: 80,
        rows: 18,
    });
    engine.process(b"\x1b[H\x1b[JPRELAUNCH_ONLY\r\n");
    let grid = engine.snapshot();
    let text: String = grid.lines.iter().flatten().map(|cell| cell.c).collect();
    assert_eq!(text.matches("BUILD_ONLY").count(), 1, "{text}");
    assert_eq!(text.matches("PRELAUNCH_ONLY").count(), 1, "{text}");
}

/// The first ConPTY inheritance query must reuse the entire restored prompt, including folded rows.
#[test]
fn restored_wrapped_prompt_inherits_its_logical_start() {
    let mut engine = Engine::new(
        GridSize {
            columns: 16,
            rows: 12,
        },
        100,
    );
    let prompt = "PS C:\\长路径\\nested-directory> ";
    engine.process(prompt.as_bytes());
    engine.begin_process(true, Some(prompt));
    let events = engine.process(b"\x1b[6n");
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::PtyWrite(reply) if reply == "\x1b[1;1R"))
    );
    engine.process(prompt.as_bytes());
    engine.resize(GridSize {
        columns: 80,
        rows: 12,
    });
    let grid = engine.snapshot();
    let text: String = grid
        .lines
        .iter()
        .flatten()
        .filter(|cell| {
            !cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
        })
        .map(|cell| cell.c)
        .collect();
    assert_eq!(text.trim_end(), prompt.trim_end());
}

/// A restored prompt may begin in history when its last wrapped rows fill a short viewport.
#[test]
fn restored_prompt_in_history_is_not_duplicated() {
    let mut engine = Engine::new(
        GridSize {
            columns: 16,
            rows: 2,
        },
        100,
    );
    let prompt = "PS C:\\long-directory\\nested-directory\\项目> ";
    engine.process(b"EARLIER OUTPUT\r\n");
    engine.process(prompt.as_bytes());
    assert!(engine.history() > 0);
    engine.begin_process(true, Some(prompt));
    let events = engine.process(b"\x1b[6n");
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::PtyWrite(reply) if reply == "\x1b[1;1R"))
    );
    engine.process(prompt.as_bytes());
    engine.resize(GridSize {
        columns: 100,
        rows: 12,
    });
    let grid = engine.snapshot();
    let text: String = grid
        .lines
        .iter()
        .flatten()
        .filter(|cell| {
            !cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
        })
        .map(|cell| cell.c)
        .collect();
    assert_eq!(text.matches("EARLIER OUTPUT").count(), 1);
    assert_eq!(text.matches("PS C:").count(), 1);
    // The inheritance boundary is under test here; subsequent native resize redraws are exercised
    // by the real long-path PowerShell upgrade test rather than synthesized as ordinary output.
    assert!(text.contains("项目>"));
}

/// Clearing display/history must not change the protocol a still-running TUI expects.
#[test]
fn clear_preserves_active_program_modes() {
    let mut engine = Engine::new(
        GridSize {
            columns: 80,
            rows: 12,
        },
        100,
    );
    engine.process(b"\x1b[?1049h\x1b[?1h\x1b[?2004h\x1b[?1000hvisible");
    let modes = engine.mode();
    engine.clear();
    assert_eq!(engine.mode(), modes);
    assert_eq!(engine.history(), 0);
    assert!(
        engine
            .snapshot()
            .lines
            .iter()
            .flatten()
            .all(|cell| cell.c == ' ')
    );
}

/// A wide glyph's spacer is printed content, whereas padding immediately after it is not.
#[test]
fn trailing_wide_character_is_selectable_from_both_halves() {
    let mut engine = Engine::new(
        GridSize {
            columns: 80,
            rows: 12,
        },
        100,
    );
    engine.process("中".as_bytes());
    engine.select(0, 1, true, 2);
    assert_eq!(engine.selected_text().as_deref(), Some("中"));
    engine.select(0, 2, true, 2);
    assert!(engine.selected_text().is_none());
}
/// Restoring trimmed default cells must not inherit a new process's active background color.
#[test]
fn restored_history_keeps_default_background_but_preserves_live_rendition() {
    let size = GridSize {
        columns: 40,
        rows: 12,
    };
    let mut engine = Engine::new(size, 100);
    for _ in 0..62 {
        engine.process(b"old\r\n");
    }
    let original = engine.snapshot();
    assert!(original.lines.len() - original.rows > original.rows);
    let original_lines = original.lines.clone();
    engine.process(b"\x1b[41m");
    engine.restore(original).unwrap();
    let restored = engine.snapshot();
    assert!(
        restored.lines == original_lines,
        "old cell colors changed during restore"
    );
    // The old rows retain their own colors, while subsequent output still uses the live SGR.
    engine.process(b"new");
    let cursor = engine.term.grid().cursor.point;
    let cell = &engine.term.grid()[Point::new(cursor.line, Column(cursor.column.0 - 1))];
    assert_ne!(cell.bg, Cell::default().bg);
}
