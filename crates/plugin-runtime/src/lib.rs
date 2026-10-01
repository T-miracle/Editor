//! Sandboxed component execution and transactional local package lifecycle.
mod capabilities;
mod instance;
mod manager;
mod migration;
mod package;
mod process;
pub use instance::Instance;
pub use manager::{Installed, Manager};
pub use package::Package;
pub use plugin_protocol;
