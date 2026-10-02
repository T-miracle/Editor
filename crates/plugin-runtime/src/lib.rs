//! Sandboxed component execution and transactional local package lifecycle.
mod capabilities;
mod document_events;
mod editor_requests;
pub use document_events::DocumentEvents;
pub use editor_requests::EditorRequest;
mod instance;
mod language_service;
pub use language_service::{LanguageService, ServiceProcess};
mod manager;
mod migration;
mod package;
mod process;
mod toolchains;
pub use instance::Instance;
pub use manager::{Installed, Manager};
pub use package::Package;
pub use plugin_protocol;
