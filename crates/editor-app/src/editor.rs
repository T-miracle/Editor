//! Document lifecycle and editor-area presentation.

mod documents;
mod view;

pub(crate) use documents::{attach_language_server, detach_language_server};
