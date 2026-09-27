//! Sandboxed component execution and transactional local package lifecycle.
mod instance;
mod manager;
mod package;
mod process;
pub use instance::Instance;
pub use manager::{Installed, Manager};
pub use package::Package;
pub use plugin_protocol;
