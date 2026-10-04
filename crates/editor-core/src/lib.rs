//! Product-agnostic editor rules.

mod commands;
mod documents;
mod run;
mod workspace;

pub use commands::{CommandDescriptor, CommandId, CommandRegistry, KeyBindingDescriptor};
pub use documents::{DocumentError, DocumentSession, DocumentStore, OpenedDocument};
pub use run::{
    BreakpointError, DebugCapabilities, DebugControls, DebugSessionState, DebugStep,
    DiscoveryOutcome, MAX_BREAKPOINT_SOURCE_BYTES, MAX_RUN_ARGUMENTS, MAX_RUN_BREAKPOINTS,
    MAX_RUN_CONFIGS, MAX_RUN_STEPS, RUN_CONFIG_VERSION, RunBreakpoint, RunBreakpoints, RunConfig,
    RunConfigError, RunConfigReadiness, RunConfigSet, RunConfigSource, RunStep, RunStoreError,
    RunTarget, SHARED_CONFIG_VERSION, SharedConfig, SharedSet, SharedStoreError, StepTarget,
    WORKSPACE_TOKEN, configuration_for, default_root, launch_environment, load, load_shared, merge,
    project_path, reconcile, repair, save, save_shared, storage_path,
};
pub use workspace::{Workspace, WorkspaceError, WorkspaceFile, WorkspaceSnapshot};
