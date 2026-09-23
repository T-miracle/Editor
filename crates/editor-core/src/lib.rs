//! Product-agnostic editor rules.

mod commands;
mod documents;
mod workspace;

pub use commands::{CommandDescriptor, CommandId, CommandRegistry, KeyBindingDescriptor};
pub use documents::{DocumentError, DocumentSession, DocumentStore, OpenedDocument};
pub use workspace::{Workspace, WorkspaceError, WorkspaceFile};
