//! Document lifecycle and editor-area presentation.

mod documents;
#[cfg(test)]
mod hover_hit_test;
mod pointer_hover;
mod popovers;
mod view;

pub(crate) use documents::{attach_language_server, detach_language_server};
