//! Plugin language registration, navigation, and external tool resolution.

pub(crate) mod code_highlighting;
pub(crate) mod completion;
pub(crate) mod diagnostics;
pub(crate) mod hover;
pub(crate) mod navigation;
pub mod plugins;
pub(crate) mod providers;

// Actual XML packages enter through the same manager and language registry as installed plugins.
#[cfg(test)]
mod xml_tests;
