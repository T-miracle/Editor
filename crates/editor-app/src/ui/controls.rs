//! Editor-owned visual controls built over gpui-base behavior.

mod button;
mod checkbox;
mod definition_popup;
mod diagnostic_popup;
pub(crate) mod dialog;
mod disclosure;
pub(crate) mod dock;
mod editor_canvas;
mod icon;
mod input;
pub(crate) mod menu;
mod notification;
mod scrollbar;
mod segmented_tabs;
mod shortcut_panel;
pub(crate) mod side_tabs;
mod spinner;
mod split;
mod status_bar;
mod status_icon;
mod tabs;
mod text;
mod textarea;
mod tooltip;
mod tree;

pub(crate) use button::{Button, ButtonCustomVariant};
pub(crate) use checkbox::Checkbox;
pub(crate) use definition_popup::definition_popup;
pub(crate) use diagnostic_popup::diagnostic_popup;
pub(crate) use dialog::DialogContent;
pub(crate) use disclosure::disclosure;
pub(crate) use editor_canvas::empty_editor_canvas;
pub(crate) use icon::Icon;
pub(crate) use input::Input;
pub(crate) use notification::Notification;
pub(crate) use scrollbar::{
    install_scrollbar_theme, vertical_scrollbar, vertical_viewport_scrollbar,
};
pub(crate) use segmented_tabs::SegmentedTabs;
pub(crate) use shortcut_panel::{
    shortcut_footer, shortcut_keycaps, shortcut_list, shortcut_modal, shortcut_row,
    shortcut_search, shortcut_tabs,
};
pub(crate) use spinner::Spinner;
pub(crate) use split::split_container;
pub(crate) use status_bar::StatusBar;
pub(crate) use status_icon::StatusIcon;
pub(crate) use tabs::tab_strip;
pub(crate) use text::{RichTextColors, markdown_view, rich_text_view};
pub(crate) use textarea::Textarea;
pub(crate) use tooltip::Tooltip;
pub(crate) use tree::tree_row;
