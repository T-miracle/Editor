//! Product-agnostic editor rules.

mod commands;
mod documents;
mod run;
mod workspace;

pub use commands::{CommandDescriptor, CommandId, CommandRegistry, KeyBindingDescriptor};
pub use documents::{DocumentError, DocumentSession, DocumentStore, OpenedDocument};
pub use run::{
    MAX_RUN_ARGUMENTS, MAX_RUN_CONFIGS, MAX_RUN_STEPS, RUN_CONFIG_VERSION, RunConfig,
    RunConfigError, RunConfigReadiness, RunConfigSet, RunConfigSource, RunStep, RunStoreError,
    RunTarget, SHARED_CONFIG_VERSION, SharedConfig, SharedSet, SharedStoreError, StepTarget,
    WORKSPACE_TOKEN, default_root, launch_environment, load, load_shared, merge, project_path,
    save, save_shared, storage_path,
};
pub use workspace::{Workspace, WorkspaceError, WorkspaceFile, WorkspaceSnapshot};
