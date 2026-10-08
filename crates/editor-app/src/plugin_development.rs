//! Built-in plugin development workflows share the runtime's project builder with the CLI.
mod cli;
pub(crate) mod configuration;
pub(crate) mod instance;
pub(crate) mod jobs;
#[cfg(test)]
mod tests;
pub(crate) use cli::run_cli;
use std::path::{Path, PathBuf};

/// Shipped plugin locations are executable-relative; source builds have a repository fallback.
pub(crate) fn shipped_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            #[cfg(target_os = "macos")]
            roots.push(parent.join("../Resources/plugins"));
            roots.push(parent.join("plugins"));
        }
    }
    if roots.iter().any(|root| root.is_dir()) {
        return roots;
    }
    roots.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins"));
    roots
}

/// One stable, private profile per workspace/configuration; never stores data in the project.
pub(crate) fn profile(workspace: &str, configuration: &str) -> anyhow::Result<PathBuf> {
    use sha2::{Digest, Sha256};
    let key = serde_json::to_vec(&(workspace, configuration))?;
    Ok(dirs::config_dir()
        .ok_or_else(|| anyhow::anyhow!("User configuration directory unavailable"))?
        .join("MeEditor/plugin-development")
        .join(format!("{:x}", Sha256::digest(key))))
}
