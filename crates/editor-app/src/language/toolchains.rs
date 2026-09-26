//! Resolves declarative plugin tool commands to executable paths.

use anyhow::{Context as _, bail};
use plugin_schema::LanguageContribution;
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// Finds a working language server executable without a shell or installation step.
pub fn resolve_server_executable(language: &LanguageContribution) -> anyhow::Result<PathBuf> {
    let command = language
        .lsp_command
        .as_deref()
        .context("language plugin does not declare an LSP executable")?;
    let mut candidates = Vec::new();
    if let Some(home) = dirs::home_dir() {
        for pattern in &language.lsp_search_paths {
            let expanded = pattern.replace("${HOME}", &home.to_string_lossy());
            let mut matches = glob::glob(&expanded)
                .with_context(|| format!("invalid LSP executable pattern {pattern}"))?
                .filter_map(Result::ok)
                .collect::<Vec<_>>();
            // Newer editor extensions usually carry the newest server binary.
            matches.sort_unstable_by(|left, right| right.cmp(left));
            candidates.extend(matches);
        }
    }
    // Resolve PATH entries to absolute executables before starting a tool.
    if let Some(paths) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&paths) {
            let base = directory.join(command);
            candidates.push(base.clone());
            if Path::new(command).extension().is_none() {
                candidates.push(base.with_extension(std::env::consts::EXE_EXTENSION));
            }
        }
    }

    for candidate in candidates {
        if !candidate.is_file() {
            continue;
        }
        if !language.lsp_check_args.is_empty() {
            // A broken toolchain shim can spawn but exit before the LSP handshake.
            let Ok(status) = Command::new(&candidate)
                .args(&language.lsp_check_args)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
            else {
                continue;
            };
            if !status.success() {
                continue;
            }
        }
        return Ok(candidate.canonicalize().unwrap_or(candidate));
    }
    bail!(
        "no working LSP executable found for {} (command: {command})",
        language.id
    )
}
