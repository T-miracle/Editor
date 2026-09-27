//! Regression probe for checkpointing and uninstalling a terminal with substantial history.
use plugin_runtime::{
    Manager, Package,
    plugin_protocol::{Environment, Event},
};
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let package = Package::read(&PathBuf::from("dist/plugins/terminal.zip"))?;
    let temp = tempfile::tempdir()?;
    let env = Environment {
        workspace: std::env::current_dir()?.display().to_string(),
        os: std::env::consts::OS.into(),
        ..Environment::default()
    };
    let mut manager = Manager::open(temp.path().to_owned(), env)?;
    manager.install(&package, package.manifest.permissions.clone())?;
    let id = &package.manifest.id;
    manager.event(
        id,
        Event::Resize {
            width: 2300.,
            height: 600.,
            cell_width: 8.,
            cell_height: 20.,
        },
    )?;
    // PTY polling delivers bounded output chunks; accumulate a full default history.
    let wide = std::env::var_os("DIAG_WIDE").is_some();
    let count = 10000;
    let chunk = if wide { 200 } else { 500 };
    for start in (0..count).step_by(chunk) {
        let bytes = (start..start + chunk)
            .map(|line| {
                if wide {
                    format!("line {line} {}\r\n", "x".repeat(200))
                } else {
                    format!("line {line}\r\n")
                }
            })
            .collect::<String>()
            .into_bytes();
        manager.event(id, Event::ProcessOutput { handle: 1, bytes })?;
    }
    manager.checkpoint()?;
    manager.uninstall(id, false)?;
    println!("PASS: history checkpoint and uninstall");
    Ok(())
}
