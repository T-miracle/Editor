//! File commands have a tree-only key context, independent from editor text commands.

use gpui_kit::{App, Global, KeyBinding, actions};

actions!(explorer_files, [PasteFiles, UndoFiles, RedoFiles]);

struct ExplorerKeymap;
impl Global for ExplorerKeymap {}

/// Each native application/test context installs its keymap once, after base controls initialize.
pub(crate) fn init(cx: &mut App) {
    if cx.has_global::<ExplorerKeymap>() {
        return;
    }
    cx.bind_keys([KeyBinding::new("ctrl-v", PasteFiles, Some("ExplorerFiles"))]);
    cx.bind_keys([
        KeyBinding::new("ctrl-z", UndoFiles, Some("ExplorerFiles")),
        KeyBinding::new("ctrl-shift-z", RedoFiles, Some("ExplorerFiles")),
    ]);
    #[cfg(target_os = "macos")]
    cx.bind_keys([KeyBinding::new("cmd-v", PasteFiles, Some("ExplorerFiles"))]);
    #[cfg(target_os = "macos")]
    cx.bind_keys([
        KeyBinding::new("cmd-z", UndoFiles, Some("ExplorerFiles")),
        KeyBinding::new("cmd-shift-z", RedoFiles, Some("ExplorerFiles")),
    ]);
    cx.set_global(ExplorerKeymap);
}
