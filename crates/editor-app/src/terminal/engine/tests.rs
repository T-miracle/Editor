//! Parser-boundary regressions supplement the real Shell/TUI editor integration tests.

use super::*;

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
