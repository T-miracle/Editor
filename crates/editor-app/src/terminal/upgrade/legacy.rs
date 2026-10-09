//! Data-only conversion of the retired guest snapshot; upstream Alacritty creates native logical cells.
use crate::terminal::{
    config::{Profile, Settings},
    engine::{Engine, GridSize, SavedGrid},
    persistence::{Saved, SavedSession, bounded_bytes},
    shell, tasks,
};
use plugin_runtime::plugin_protocol::Snapshot;
use serde::Deserialize;
use std::collections::BTreeSet;

#[derive(Deserialize)]
struct GuestState {
    tabs: Vec<GuestTab>,
    active: usize,
    next_id: u64,
    settings: Settings,
    #[serde(default = "width")]
    tab_width: f32,
    #[serde(default)]
    recovery_version: u32,
}
fn width() -> f32 {
    180.
}
#[derive(Deserialize)]
struct GuestTab {
    id: u64,
    name: String,
    profile: Profile,
    cwd: String,
    output: String,
    #[serde(default)]
    exited: bool,
    #[serde(default)]
    display: Option<Display>,
}
#[derive(Deserialize)]
struct Display {
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

/// Validate all old records before returning any new state; never send transcript bytes to a program.
pub(super) fn convert(bytes: &[u8], external: Option<&[u8]>) -> anyhow::Result<Vec<u8>> {
    let snapshot: Snapshot = serde_json::from_slice(bytes)?;
    anyhow::ensure!(
        matches!(snapshot.schema, 1 | 2) && snapshot.data.len() <= 16 * 1024 * 1024,
        "Unsupported terminal snapshot"
    );
    let old: GuestState = serde_json::from_str(&snapshot.data)?;
    let settings = Settings::parse(&serde_json::to_string(&old.settings)?)?;
    let settings = if let Some(bytes) = external {
        Settings::parse(std::str::from_utf8(bytes)?)?
    } else {
        settings
    };
    let ids = old.tabs.iter().map(|tab| tab.id).collect::<BTreeSet<_>>();
    anyhow::ensure!(
        old.tabs.len() <= 32
            && ids.len() == old.tabs.len()
            && !ids.contains(&0)
            && ids.last().is_none_or(|id| *id <= old.next_id)
            && old.next_id < u64::MAX
            && old.tab_width.is_finite(),
        "Invalid terminal identities or layout"
    );
    let active = old
        .tabs
        .get(old.active)
        .map(|tab| tab.id)
        .or_else(|| old.tabs.first().map(|tab| tab.id));
    let mut sessions = Vec::new();
    for tab in old.tabs {
        anyhow::ensure!(
            tab.name.chars().count() <= 256
                && !tab.name.chars().any(char::is_control)
                && !tab.cwd.contains('\0')
                && tab.cwd.len() <= 4096
                && !tab.profile.program.contains('\0')
                && tab.profile.program.len() <= 4096
                && !tab.profile.program.is_empty()
                && tab.profile.args.len() <= 128
                && tab
                    .profile
                    .args
                    .iter()
                    .all(|arg| arg.len() <= 32768 && !arg.contains('\0')),
            "Invalid terminal profile"
        );
        let mut grid = restore_grid(&tab, settings.history)?;
        if old.recovery_version == 0
            && let Some(prompt) = shell::default_prompt(&tab.profile, &tab.cwd)
        {
            repair_generated_gap(&mut grid, &prompt);
        }
        // The old service-created profile is a finite format discriminator, not a live provider ID.
        let task = if tab.profile.name == "Service execution" {
            Some(serde_json::from_value::<tasks::Task>(
                serde_json::json!({"key":format!("legacy-task:{}",tab.id),"debug":false}),
            )?)
        } else {
            None
        };
        sessions.push(SavedSession {
            id: tab.id,
            name: tab.name,
            profile: tab.profile,
            cwd: tab.cwd,
            exited: tab.exited || task.is_some(),
            grid,
            task,
        });
    }
    bounded_bytes(&mut Saved {
        version: 1,
        settings,
        active,
        next_id: old.next_id,
        tab_width: old.tab_width.clamp(112., 480.),
        sessions,
    })
}

/// Saved physical wrap markers join rows before replay; modern snapshots already contain soft wraps.
fn restore_grid(tab: &GuestTab, history: usize) -> anyhow::Result<SavedGrid> {
    let size = if let Some(display) = &tab.display {
        anyhow::ensure!(
            (1..=500).contains(&display.rows)
                && (2..=1000).contains(&display.columns)
                && display.cursor.0 < display.rows
                && display.cursor.1 < display.columns
                && display.wrapped_lines.len() <= history + 500,
            "Invalid terminal geometry"
        );
        GridSize {
            rows: display.rows.into(),
            columns: display.columns.into(),
        }
    } else {
        GridSize {
            rows: 24,
            columns: 80,
        }
    };
    let mut engine = Engine::new(size, history);
    let output = match &tab.display {
        Some(display) if !display.soft_wraps => {
            let lines: Vec<_> = tab.output.split("\r\n").collect();
            let start = i32::from(display.rows) - lines.len() as i32;
            let wrapped = display
                .wrapped_lines
                .iter()
                .copied()
                .collect::<BTreeSet<_>>();
            let mut result = String::new();
            for (index, line) in lines.iter().enumerate() {
                result.push_str(line);
                if index + 1 < lines.len() && !wrapped.contains(&(start + index as i32)) {
                    result.push_str("\r\n");
                }
            }
            result
        }
        Some(_) => tab.output.clone(),
        None => {
            // Only schema-one generated restoration banners/suffixes are removed, as in its reader.
            let marker = "\x1b[0m\x1b[0m--- restored session; new shell ---\x1b[0m\r\n";
            let text = tab
                .output
                .replace(&format!("\x1b[0m\r\n{marker}"), "")
                .replace(marker, "");
            text.strip_suffix("\r\n").unwrap_or(&text).to_owned()
        }
    };
    let _ = engine.process(output.as_bytes());
    if let Some(display) = &tab.display {
        if let Some(cursor) = &display.cursor_bytes {
            anyhow::ensure!(cursor.len() <= 8192, "Invalid terminal cursor state");
            let _ = engine.process(cursor.as_bytes());
        }
        // The saved caret is authoritative, including pending wrap. No newline is injected.
        let mut grid = engine.snapshot();
        grid.cursor = (display.cursor.0.into(), display.cursor.1.into());
        grid.input_needs_wrap = display.wrap_pending;
        grid.offset = display
            .scrollback
            .min(grid.lines.len().saturating_sub(grid.rows));
        return Ok(grid);
    }
    Ok(engine.snapshot())
}

/// Version zero alone may have an empty gap between identical, unwrapped default PowerShell prompts.
fn repair_generated_gap(grid: &mut SavedGrid, prompt: &str) {
    use alacritty_terminal::term::cell::Flags;
    let cursor = grid.lines.len() - grid.rows + grid.cursor.0 as usize;
    let text = |line: &Vec<alacritty_terminal::term::cell::Cell>| {
        line.iter().map(|cell| cell.c).collect::<String>()
    };
    let empty = |line: &Vec<alacritty_terminal::term::cell::Cell>| {
        line.iter()
            .all(|cell| cell == &alacritty_terminal::term::cell::Cell::default())
    };
    if text(&grid.lines[cursor]).trim_end() != prompt.trim_end()
        || grid.lines[cursor]
            .iter()
            .any(|cell| cell.flags.contains(Flags::WRAPLINE))
        || grid.lines.iter().skip(cursor + 1).any(|line| !empty(line))
    {
        return;
    }
    let Some(previous) = (grid.lines.len() - grid.rows..cursor)
        .rev()
        .find(|index| !empty(&grid.lines[*index]))
    else {
        return;
    };
    let gap = cursor - previous - 1;
    if gap == 0
        || text(&grid.lines[previous]).trim_end() != prompt.trim_end()
        || grid.lines[previous]
            .iter()
            .any(|cell| cell.flags.contains(Flags::WRAPLINE))
    {
        return;
    }
    grid.lines.drain(previous + 1..cursor);
    grid.lines.extend((0..gap).map(|_| vec![]));
    grid.cursor.0 -= gap as i32;
}
