//! Canonical one/two-stroke capture shared by key search and inline binding drafts.

use gpui_kit::Keystroke;
use std::time::{Duration, Instant};

/// A deadline belongs to the first stroke, never to the most recent repaint.
#[derive(Default)]
pub(super) struct Capture {
    pub strokes: Vec<String>,
    pub waiting: bool,
    pub generation: u64,
    deadline: Option<Instant>,
}

impl Capture {
    /// Invalidate timers across recording sessions instead of reusing their generation numbers.
    pub fn clear(&mut self) {
        self.strokes.clear();
        self.waiting = false;
        self.deadline = None;
        self.generation += 1;
    }

    /// Record real keys with exact modifiers; modifier-only events do not form a stroke.
    pub fn record(&mut self, key: &Keystroke, now: Instant) -> bool {
        if matches!(key.key.as_str(), "shift" | "control" | "alt" | "cmd" | "fn")
            || key.key.is_empty()
        {
            return false;
        }
        if self.waiting && self.deadline.is_some_and(|deadline| now < deadline) {
            self.strokes.push(key.unparse());
            self.waiting = false;
            self.deadline = None;
        } else {
            self.strokes = vec![key.unparse()];
            self.waiting = true;
            self.deadline = Some(now + Duration::from_secs(2));
        }
        self.generation += 1;
        true
    }
}

/// Convert canonical binding text into local keycap labels without changing stored steps.
pub(super) fn display(strokes: &[String]) -> Vec<String> {
    strokes
        .iter()
        .map(|stroke| {
            let Ok(key) = Keystroke::parse(stroke) else {
                return stroke.clone();
            };
            let mut parts = Vec::new();
            if key.modifiers.control {
                parts.push("Ctrl".to_owned());
            }
            if key.modifiers.alt {
                parts.push("Alt".to_owned());
            }
            if key.modifiers.shift {
                parts.push("Shift".to_owned());
            }
            if key.modifiers.platform {
                parts.push("Cmd".to_owned());
            }
            if key.modifiers.function {
                parts.push("Fn".to_owned());
            }
            parts.push(match key.key.as_str() {
                "left" => "←".into(),
                "right" => "→".into(),
                "up" => "↑".into(),
                "down" => "↓".into(),
                "escape" => "Esc".into(),
                "space" => "Space".into(),
                "enter" => "Enter".into(),
                _ => key.key.to_uppercase(),
            });
            parts.join("+")
        })
        .collect()
}
