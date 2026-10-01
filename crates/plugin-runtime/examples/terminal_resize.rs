//! Regression for a restored PowerShell transcript that gains a blank row on its first resize.
use plugin_runtime::{
    Manager, Package,
    plugin_protocol::{Environment, Event, Paint},
};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

/// Reassemble visible text cells, keeping real row positions so an inserted blank is observable.
fn rows(manager: &Manager, id: &str) -> Vec<(i32, String)> {
    let mut rows = BTreeMap::<i32, String>::new();
    for paint in &manager.live[id].scene.as_ref().unwrap().paint {
        if let Paint::Text { y, text, .. } = paint {
            rows.entry(y.round() as i32).or_default().push_str(text);
        }
    }
    rows.into_iter().collect()
}

/// Include the host's 150 ms resize delay and the subsequent asynchronous ConPTY repaint.
fn settle(manager: &mut Manager, id: &str) -> anyhow::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(4);
    let mut previous = Vec::new();
    let mut changed = Instant::now();
    while Instant::now() < deadline {
        manager.poll();
        anyhow::ensure!(
            manager.installed[id].error.is_none(),
            "Runtime error: {:?}",
            manager.installed[id].error
        );
        let current = rows(manager, id);
        if current != previous {
            previous = current;
            changed = Instant::now();
        }
        if !previous.is_empty() && changed.elapsed() > Duration::from_millis(400) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    anyhow::bail!("PowerShell output did not settle within four seconds")
}

/// Send the same logical panel dimensions and font metrics as the editor's native canvas.
fn resize(manager: &mut Manager, id: &str, height: f32) -> anyhow::Result<()> {
    manager.event(
        id,
        Event::Resize {
            width: 1600.,
            height,
            cell_width: 8.4,
            cell_height: 21.,
        },
    )
}

/// A single command and its one-line output must be followed immediately by the current prompt.
fn check_spacing(manager: &Manager, id: &str) -> anyhow::Result<()> {
    let current = rows(manager, id);
    anyhow::ensure!(
        current.len() == 3,
        "Missing or duplicated command/output/prompt: {current:?}"
    );
    anyhow::ensure!(
        current[0].1.ends_with("Write-OutputRESIZE_OUTPUT"),
        "Command was changed: {current:?}"
    );
    anyhow::ensure!(
        current[1].1 == "RESIZE_OUTPUT",
        "Output was changed: {current:?}"
    );
    anyhow::ensure!(
        current[2].1.starts_with("PS") && current[2].1.ends_with('>'),
        "Prompt was changed: {current:?}"
    );
    anyhow::ensure!(
        current.windows(2).all(|pair| pair[1].0 - pair[0].0 == 21),
        "Blank rows between command, output and prompt: {current:?}"
    );
    Ok(())
}

/// Restore actual command output before resizing; --fresh provides a non-restoring comparison.
fn main() -> anyhow::Result<()> {
    if !cfg!(windows) {
        println!("SKIP: this regression exercises Windows ConPTY and PowerShell");
        return Ok(());
    }
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    let path = arguments
        .iter()
        .find(|arg| !arg.starts_with("--"))
        .map(String::as_str)
        .unwrap_or("dist/plugins/terminal.zip");
    let restore = !arguments.iter().any(|arg| arg == "--fresh");
    let minimal = arguments.iter().any(|arg| arg == "--minimal");
    let package = Package::read(std::path::Path::new(path))?;
    let id = &package.manifest.id;
    let temp = tempfile::tempdir()?;
    let environment = Environment {
        workspace: std::env::current_dir()?.display().to_string(),
        os: std::env::consts::OS.into(),
        ..Environment::default()
    };
    let mut manager = Manager::open(temp.path().to_owned(), environment.clone())?;
    manager.install(&package, package.manifest.permissions.clone())?;
    resize(&mut manager, id, 310.)?;
    settle(&mut manager, id)?;
    // This built-in command matches node -v's three-row transcript without requiring Node.
    manager.event(id, Event::Text("Write-Output RESIZE_OUTPUT\r".into()))?;
    settle(&mut manager, id)?;
    check_spacing(&manager, id)?;
    if restore {
        manager.checkpoint()?;
        drop(manager);
        manager = Manager::open(temp.path().to_owned(), environment)?;
        resize(&mut manager, id, 310.)?;
        settle(&mut manager, id)?;
    }
    println!(
        "before resize version={} restored={restore}: {:?}",
        package.manifest.version,
        rows(&manager, id)
    );
    check_spacing(&manager, id)?;
    let heights: &[f32] = if minimal {
        &[500.]
    } else {
        &[500., 200., 380., 140., 620., 310.]
    };
    for round in 0..if minimal { 1 } else { 3 } {
        for &height in heights {
            resize(&mut manager, id, height)?;
            // Settling each size prevents resize coalescing from bypassing the actual bug.
            settle(&mut manager, id)?;
            println!("round {round}, height {height}: {:?}", rows(&manager, id));
            check_spacing(&manager, id)?;
        }
    }
    if !minimal {
        for _ in 0..6 {
            for &height in heights {
                resize(&mut manager, id, height)?;
                manager.poll();
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        // Also cover fast drag coalescing, followed by submission of another real command.
        settle(&mut manager, id)?;
        check_spacing(&manager, id)?;
        manager.event(id, Event::Text("Write-Output RESIZE_OUTPUT\r".into()))?;
        settle(&mut manager, id)?;
        let current = rows(&manager, id);
        anyhow::ensure!(
            current.len() == 5 && current.windows(2).all(|pair| pair[1].0 - pair[0].0 == 21),
            "Resize displaced subsequent command output: {current:?}"
        );
    }
    manager.uninstall(id, true)?;
    println!("PASS: real height changes preserve command output and prompt spacing");
    Ok(())
}
