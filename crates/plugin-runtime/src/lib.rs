//! Sandboxed component execution and transactional local package lifecycle.
mod capabilities;
mod dependencies;
pub use dependencies::{InstallControl, InstallStage, InstallerPrompt};
mod data_transaction;
mod document_events;
mod editor_requests;
mod images;
pub use images::{ImageResource, ImageState};
mod image_input;
pub use image_input::{
    HostImageInput, HostImageOrigin, IMAGE_INPUT_MAX_BATCH, IMAGE_INPUT_MAX_BATCH_BYTES,
    IMAGE_INPUT_MAX_BYTES, IMAGE_INPUT_MAX_RESIDENT_BYTES, IMAGE_INPUT_TIMEOUT_MS,
    ImageInputResource,
};
pub mod faults;
pub mod logs;
pub use logs::{LogLevel, LogRecord, RuntimeLogs};
mod request_state;
pub use document_events::DocumentEvents;
pub use editor_requests::EditorRequest;
mod instance;
mod language_service;
pub use language_service::{LanguageService, ServiceProcess};
mod manager;
mod migration;
mod package;
mod plugin_services;
mod process;
mod toolchains;
pub use instance::Instance;
pub use manager::{
    EXECUTION_CONTRACT, EXECUTION_START_TIMEOUT_MS, ExecutionFailure, ExecutionSnapshot,
    ExecutionState, HostExecution, InstallationPreparation, Installed, Manager,
    PreparedInstallation, RunEnvEntry, RunRequest,
};
pub use package::Package;
pub use plugin_protocol;

/// Host-owned resources travel with every candidate; guests only see negotiated descriptions.
#[derive(Clone, Debug, Default)]
pub struct HostResources {
    /// No SDK is a supported host configuration; an export failure affects only a requesting guest.
    pub sdk: Option<Result<plugin_protocol::api::SdkDescriptor, String>>,
    /// One process-local log owner is shared by live, prepared and retired sources; never exposed to WASI.
    pub logs: RuntimeLogs,
}
