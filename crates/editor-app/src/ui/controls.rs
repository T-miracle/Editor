//! Editor-owned visual controls built over gpui-base behavior.

mod button;
mod checkbox;
mod definition_popup;
mod diagnostic_popup;
mod dialog;
pub(crate) mod dock;
mod icon;
mod input;
pub(crate) mod menu;
mod scrollbar;
pub(crate) mod side_tabs;
mod spinner;
mod status_bar;
mod tabs;
mod text;
mod tooltip;

pub(crate) use button::{Button, ButtonCustomVariant};
pub(crate) use checkbox::Checkbox;
pub(crate) use definition_popup::definition_popup;
pub(crate) use diagnostic_popup::diagnostic_popup;
pub(crate) use dialog::DialogContent;
pub(crate) use icon::Icon;
pub(crate) use input::Input;
pub(crate) use scrollbar::vertical_scrollbar;
pub(crate) use spinner::Spinner;
pub(crate) use status_bar::StatusBar;
pub(crate) use tabs::tab_strip;
pub(crate) use text::markdown_view;
pub(crate) use tooltip::Tooltip;
