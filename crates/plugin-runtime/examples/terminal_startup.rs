//! Run the packaged terminal through startup, resize and restore, checking prompt spacing.
use plugin_runtime::{
    Manager, Package,
    plugin_protocol::{Environment, Event, Paint},
};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

/// Reassemble the guest's cell drawing operations into prompt rows, without recording commands.
fn prompt_rows(manager: &Manager, id: &str) -> Vec<(i32, String)> {
    let scene = manager.live[id].scene.as_ref().unwrap();
    let mut rows = BTreeMap::<i32, String>::new();
    for paint in &scene.paint {
        if let Paint::Text { y, text, .. } = paint {
            rows.entry(y.round() as i32).or_default().push_str(text);
        }
    }
    rows.into_iter()
        .filter(|(_, text)| {
            // Blank cells do not produce text paint operations, so the space after PS is absent.
            text.starts_with("PS") && text.contains('>')
        })
        .collect()
}

/// Require exactly two adjacent prompts, catching both blank rows and same-line duplication.
fn check_prompts(manager: &Manager, id: &str) -> anyhow::Result<Vec<i32>> {
    let rows = prompt_rows(manager, id);
    let positions = rows.iter().map(|(y, _)| *y).collect::<Vec<_>>();
    anyhow::ensure!(rows.len() == 2, "Expected two prompt rows: {positions:?}");
    anyhow::ensure!(
        rows.iter().all(|(_, text)| text.matches('>').count() == 1),
        "A restored prompt was appended to an existing prompt"
    );
    anyhow::ensure!(
        positions[1] - positions[0] == 21,
        "Prompts have blank rows between them: {positions:?}"
    );
    Ok(positions)
}

/// Pump real ConPTY output until the prompt has settled, with a finite startup timeout.
fn settle(manager: &mut Manager, id: &str) {
    let deadline = Instant::now() + Duration::from_secs(4);
    let mut revision = 0;
    let mut changed = Instant::now();
    while Instant::now() < deadline {
        manager.poll();
        let next = manager.live[id]
            .scene
            .as_ref()
            .unwrap()
            .controls
            .as_ref()
            .unwrap()
            .revision;
        if next != revision {
            revision = next;
            changed = Instant::now();
        }
        if !prompt_rows(manager, id).is_empty() && changed.elapsed() > Duration::from_millis(400) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Opening uses the same full-panel resize event as the editor's native canvas.
fn resize(manager: &mut Manager, id: &str, width: f32, height: f32) -> anyhow::Result<()> {
    manager.event(
        id,
        Event::Resize {
            width,
            height,
            cell_width: 8.4,
            cell_height: 21.,
        },
    )
}

/// Check the Windows inherited-cursor handshake before and after several viewport changes.
fn main() -> anyhow::Result<()> {
    if !cfg!(windows) {
        println!("SKIP: this regression exercises Windows ConPTY and PowerShell");
        return Ok(());
    }
    let path = std::env::args()
        .nth(1)
        .unwrap_or("dist/plugins/terminal.zip".into());
    let package = Package::read(std::path::Path::new(&path))?;
    let id = &package.manifest.id;
    for immediate_resize in [true, false] {
        let temp = tempfile::tempdir()?;
        let env = Environment {
            workspace: std::env::current_dir()?.display().to_string(),
            os: std::env::consts::OS.into(),
            ..Environment::default()
        };
        let mut manager = Manager::open(temp.path().to_owned(), env.clone())?;
        manager.install(&package, package.manifest.permissions.clone())?;
        if immediate_resize {
            resize(&mut manager, id, 1600., 380.)?;
        }
        settle(&mut manager, id);
        if !immediate_resize {
            resize(&mut manager, id, 1600., 380.)?;
            settle(&mut manager, id);
        }
        anyhow::ensure!(
            prompt_rows(&manager, id).len() == 1,
            "Expected one fresh prompt"
        );
        // Retain one previous prompt and one current prompt, as an ordinary Enter would.
        manager.event(id, Event::Text("\r".into()))?;
        settle(&mut manager, id);
        let fresh = check_prompts(&manager, id)?;
        println!("saved immediate_resize={immediate_resize}: {fresh:?}");
        manager.checkpoint()?;
        drop(manager);
        // Reopen real persisted snapshots at the same size, shorter height and changed widths.
        for (width, height) in [(1600., 380.), (1600., 310.), (2300., 310.), (1000., 620.)] {
            let mut manager = Manager::open(temp.path().to_owned(), env.clone())?;
            resize(&mut manager, id, width, height)?;
            settle(&mut manager, id);
            let restored = check_prompts(&manager, id)?;
            println!(
                "restore immediate_resize={immediate_resize} size={width}x{height}: {restored:?}"
            );
            manager.checkpoint()?;
        }
        let mut manager = Manager::open(temp.path().to_owned(), env)?;
        manager.uninstall(id, true)?;
    }
    println!("PASS: fresh and restored prompt spacing");
    Ok(())
}
