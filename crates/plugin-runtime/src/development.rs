//! One build/validation surface serves command-line packaging and isolated development hosts.
//! Projects describe inputs explicitly; source trees and old distribution scripts are never archived.
mod build;
mod process;
mod project;
#[cfg(test)]
mod tests;
pub use build::{read_directory, stage};
pub use process::{HostProcess, Stream, Update};
pub use project::DESCRIPTION;
pub use project::{Asset, BuildStep, DescriptionReadError, Project, ProjectDescription, WasmBuild};
use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};

/// Host-selected overrides. Project descriptions cannot grant trust or install missing toolchains.
#[derive(Clone, Default)]
pub struct BuildOptions {
    /// Release for distributable ZIPs, debug for development runs.
    pub release: bool,
    /// Explicit CLI/local configuration output directory; otherwise use the project's default.
    pub output: Option<PathBuf>,
    /// Native platform (for example windows-x86_64); defaults to the current OS/architecture.
    pub platform: Option<String>,
    /// Managed SDK Cargo arguments supplied by the building host.
    pub cargo_args: Vec<String>,
    /// Cancellation terminates owned build trees and never publishes a partial output.
    pub cancelled: Arc<AtomicBool>,
}

/// Platform identity used by native step declarations; WASM remains portable.
pub fn platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}
