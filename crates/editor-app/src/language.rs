//! Plugin language registration, navigation, and external tool resolution.

pub(crate) mod completion;
pub(crate) mod diagnostics;
pub(crate) mod hover;
pub(crate) mod navigation;
pub mod plugins;
pub(crate) mod providers;
mod sdk;
pub(crate) mod toolchains;
