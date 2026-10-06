//! Explicit maintenance routing for the approved one-workspace configuration cutover.
use editor_core::Workspace;

/// Handle preview/delete commands before opening any UI. The workspace is canonicalized by the
/// same public constructor as normal startup; ordinary startup neither cleans nor migrates data.
pub(crate) fn run_cli() -> anyhow::Result<bool> {
    let mut args = std::env::args_os().skip(1);
    let command = args.next();
    let command = command.as_deref().and_then(|command| command.to_str());
    if !matches!(
        command,
        Some("--preview-legacy-run-configurations" | "--clear-legacy-run-configurations")
    ) {
        return Ok(false);
    }
    let path = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("Expected one workspace directory"))?;
    anyhow::ensure!(
        args.next().is_none(),
        "Only one explicit workspace is accepted"
    );
    let workspace = Workspace::open(path)?;
    let key = workspace.root().display().to_string();
    let root =
        editor_core::default_root().ok_or_else(|| anyhow::anyhow!("Run storage is unavailable"))?;
    let paths = if command == Some("--clear-legacy-run-configurations") {
        editor_core::clear_legacy_configurations(&root, &key, workspace.root())?
    } else {
        editor_core::legacy_configuration_paths(&root, &key, workspace.root())?
    };
    println!("{}", serde_json::to_string(&paths)?);
    Ok(true)
}
