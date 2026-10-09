//! Document lifecycle and editor-area presentation.

pub(crate) mod comparison;
#[cfg(test)]
mod definition_tests;
pub(crate) mod diagnostics;
mod documents;
#[cfg(test)]
mod empty_canvas_tests;
pub(crate) mod file_watch;
#[cfg(test)]
mod file_watch_tests;
#[cfg(test)]
mod hover_hit_test;
pub(crate) mod language_edits;
pub(crate) mod linked_input;
mod pointer_hover;
mod resources;
mod source;
pub(crate) mod tabs;
mod text_session;
pub(crate) use popovers::{CompletionPopupState, DefinitionPopupFocus};
mod popovers;
pub(crate) use text_drag::TextDragState;
pub(crate) use text_drag::caret_offset_at;
mod text_drag;
mod view;
pub(crate) mod viewport;

pub(crate) use documents::{attach_language_server, detach_language_server, language_for_path};
