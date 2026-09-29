//! Export the host's compiled-in plugin contract for independently built guests.
use std::path::{Path, PathBuf};

const SDK_FILES: &[(&str, &[u8])] = &[
    (
        "src/ui/chrome.rs",
        include_bytes!("../../plugin-protocol/src/ui/chrome.rs"),
    ),
    (
        "Cargo.toml",
        include_bytes!("../../plugin-protocol/Cargo.toml"),
    ),
    (
        "README.md",
        include_bytes!("../../plugin-protocol/README.md"),
    ),
    ("UI.md", include_bytes!("../../plugin-protocol/UI.md")),
    (
        "src/lib.rs",
        include_bytes!("../../plugin-protocol/src/lib.rs"),
    ),
    (
        "src/ui.rs",
        include_bytes!("../../plugin-protocol/src/ui.rs"),
    ),
    (
        "src/ui/validate.rs",
        include_bytes!("../../plugin-protocol/src/ui/validate.rs"),
    ),
    (
        "src/ui/tests.rs",
        include_bytes!("../../plugin-protocol/src/ui/tests.rs"),
    ),
    (
        "wit/plugin.wit",
        include_bytes!("../../plugin-protocol/wit/plugin.wit"),
    ),
];

/// A packaged editor can supply its own exact contract without opening a GUI.
pub fn requested_target() -> anyhow::Result<Option<PathBuf>> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--export-plugin-sdk")) {
        return Ok(None);
    }
    let target = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("--export-plugin-sdk requires an output directory"))?;
    anyhow::ensure!(args.next().is_none(), "unexpected SDK export argument");
    Ok(Some(PathBuf::from(target)))
}

pub fn export(target: &Path) -> anyhow::Result<()> {
    for (relative, bytes) in SDK_FILES {
        let path = target.join(relative);
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(path, bytes)?;
    }
    Ok(())
}
