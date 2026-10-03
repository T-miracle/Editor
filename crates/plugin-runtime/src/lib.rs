//! Sandboxed component execution and transactional local package lifecycle.
mod capabilities;
mod dependencies;
pub use dependencies::{InstallControl, InstallStage, InstallerPrompt};
mod data_transaction;
mod document_events;
mod editor_requests;
mod images;
pub use images::{ImageResource, ImageState};
pub mod faults;
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
pub use manager::{InstallationPreparation, Installed, Manager, PreparedInstallation};
pub use package::Package;
pub use plugin_protocol;

/// Host-owned immutable inputs travel with every candidate; guests only see negotiated descriptions.
#[derive(Clone, Debug, Default)]
pub struct HostResources {
    /// No SDK is a supported host configuration; an export failure affects only a requesting guest.
    pub sdk: Option<Result<plugin_protocol::api::SdkDescriptor, String>>,
}
