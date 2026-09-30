//! Document lifecycle and editor-area presentation.

#[cfg(test)]
mod definition_tests;
pub(crate) mod diagnostics;
mod documents;
pub(crate) mod file_watch;
#[cfg(test)]
mod file_watch_tests;
#[cfg(test)]
mod hover_hit_test;
mod pointer_hover;
pub(crate) use popovers::{CompletionPopupState, DefinitionPopupFocus};
mod popovers;
mod view;

pub(crate) use documents::{attach_language_server, detach_language_server};
