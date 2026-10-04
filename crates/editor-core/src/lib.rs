//! Product-agnostic editor rules.

mod commands;
mod documents;
mod run;
mod workspace;

pub use commands::{CommandDescriptor, CommandId, CommandRegistry, KeyBindingDescriptor};
pub use documents::{DocumentError, DocumentSession, DocumentStore, OpenedDocument};
pub use run::{
    MAX_RUN_ARGUMENTS, MAX_RUN_CONFIGS, RUN_CONFIG_VERSION, RunConfig, RunConfigError,
    RunConfigReadiness, RunConfigSet, RunStoreError, RunTarget, default_root, launch_environment,
    load, save, storage_path,
};
pub use workspace::{Workspace, WorkspaceError, WorkspaceFile, WorkspaceSnapshot};
