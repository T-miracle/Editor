//! Reproduce prompt duplication while a packaged terminal receives rapid native resize events.
use plugin_runtime::{
    Manager, Package,
    plugin_protocol::{Environment, Event, Paint},
};
use std::{collections::BTreeMap, path::PathBuf, time::Duration};

fn prompt_rows(manager: &Manager, id: &str) -> Vec<String> {
    // A rendered prompt should occupy exactly one text row after the panel settles.
    let mut rows: BTreeMap<i32, Vec<(i32, &str)>> = BTreeMap::new();
    for paint in &manager.live[id].scenes["terminal"].paint {
        if let Paint::Text { x, y, text, .. } = paint {
            rows.entry(y.round() as i32)
                .or_default()
                .push((x.round() as i32, text));
        }
    }
    rows.into_values()
        .filter_map(|mut cells| {
            cells.sort_by_key(|(x, _)| *x);
            let row = cells.into_iter().map(|(_, text)| text).collect::<String>();
            // GPUI omits blank cells from Paint, so "PS C:" is joined as "PSC:".
            row.starts_with("PSC:").then_some(row)
        })
        .collect()
}

fn saved_prompt_count(manager: &mut Manager, id: &str) -> anyhow::Result<usize> {
    // Include scrollback, since a duplicate prompt may have moved above the viewport.
    let snapshot = manager.live.get_mut(id).unwrap().snapshot()?;
    let saved: serde_json::Value = serde_json::from_str(&snapshot.data)?;
    Ok(saved["tabs"][0]["output"]
        .as_str()
        .unwrap_or_default()
        .matches("PS C:")
        .count())
}

fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    let package = Package::read(&PathBuf::from(args.get(1).expect("terminal ZIP path")))?;
    let changes: usize = args
        .get(2)
        .and_then(|value| value.parse().ok())
        .unwrap_or(20);
    let pause_ms: u64 = args
        .get(3)
        .and_then(|value| value.parse().ok())
        .unwrap_or(30);
    let root = tempfile::tempdir()?;
    let environment = Environment {
        workspace: std::env::current_dir()?.display().to_string(),
        os: std::env::consts::OS.into(),
        foreground: 0xffffff,
        ..Environment::default()
    };
    let mut manager = Manager::open(root.path().to_owned(), environment)?;
    manager.install(&package, package.manifest.permissions.clone())?;
    let id = &package.manifest.id;
    for _ in 0..50 {
        manager.poll();
        if !prompt_rows(&manager, id).is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    let initial = prompt_rows(&manager, id);
    // Failed startup diagnostics include the guest-rendered screen, not only matching prompt rows.
    if initial.is_empty() {
        let screen = manager.live[id]
            .scene
            .as_ref()
            .unwrap()
            .paint
            .iter()
            .filter_map(|paint| {
                if let Paint::Text { text, .. } = paint {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect::<String>();
        println!(
            "startup screen: {screen:?}; process IDs: {:?}",
            manager.live[id].process_ids()
        );
    }
    println!("initial prompt rows: {} {initial:?}", initial.len());
    anyhow::ensure!(
        initial.len() == 1,
        "initial shell prompt did not fit one row"
    );
    // PSReadLine redraws an unsubmitted command when ConPTY changes its dimensions.
    manager.event(id, Event::Text("Write-Output 'HELLO_RESIZE_TEST'".into()))?;
    for _ in 0..10 {
        manager.poll();
        std::thread::sleep(Duration::from_millis(30));
    }
    let initial_saved = saved_prompt_count(&mut manager, id)?;
    println!("initial saved prompts: {initial_saved}");
    for index in 0..changes {
        // Drag through widths that wrap the prompt, then return to a wide final panel.
        let width = if index % 2 == 0 { 300. } else { 1100. };
        manager.event(
            id,
            Event::Resize {
                width,
                height: 420. + (index % 3) as f32 * 20.,
                cell_width: 8.,
                cell_height: 20.,
            },
        )?;
        manager.poll();
        std::thread::sleep(Duration::from_millis(pause_ms));
    }
    for _ in 0..30 {
        manager.poll();
        std::thread::sleep(Duration::from_millis(40));
    }
    let final_rows = prompt_rows(&manager, id);
    println!("final prompt rows: {} {final_rows:?}", final_rows.len());
    let final_saved = saved_prompt_count(&mut manager, id)?;
    println!("final saved prompts: {final_saved}");
    anyhow::ensure!(
        final_saved == initial_saved,
        "rapid resize added duplicate prompts to terminal history"
    );
    anyhow::ensure!(
        final_rows.len() == 1,
        "rapid resize duplicated one shell prompt into multiple rows"
    );
    Ok(())
}
