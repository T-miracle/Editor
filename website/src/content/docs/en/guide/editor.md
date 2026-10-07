---
title: Editing and shortcuts
description: Typing, selecting, moving text, folding, soft wrap and saving, with the keys that do each.
section: guide
order: 2
alternate: /zh-cn/guide/editor/
---

# Editing and shortcuts

## The editing surface

The editor shows line numbers, indent guides and a folding gutter. Folding and soft wrap are
toggles, so a document can be read without reflowing long lines or with them wrapped to the
window width.

Text is edited directly in the buffer. Undo and redo work per session and per document, and
the unsaved state is shown on the tab.

## Moving a selection

Select some text and hold the left mouse button down on it; the selection follows the pointer,
and releasing it moves the text to the new position. Dragging past the edge of the viewport
scrolls the document, and `Esc` cancels the move and leaves the text where it started. A
single undo restores the original position.

## Saving

By default, `Ctrl+S` writes the buffer to disk. Saving is what triggers the local history snapshot; the
snapshot lives in the user data directory, not in your project.

If the file changed on disk since it was read, the editor tells you instead of overwriting
silently, and keeps your unsaved edits. Saving again resolves the conflict.

## Shortcuts

These are the default bindings. You can change them in the shortcuts panel.

| Key | Action |
| --- | --- |
| `Ctrl+K` | Open the shortcuts panel |
| `Ctrl+S` | Save the active document |
| `Ctrl+Shift+R` | Refresh the file tree from disk |
| `Ctrl+Alt+T` | Switch between the light and dark theme |
| `F12` | Go to the definition of the symbol at the cursor |
| `F8` | Go to the next problem reported for this document |
| `Shift+F8` | Go to the previous problem |
| `Esc` | Cancel the current interaction, such as a pending drag or an open popup |

Middle-clicking a symbol also goes to its definition, and reports when the symbol has none.
A middle-click on a symbol with no definition briefly shows that result rather than doing
nothing.

Not every shortcut applies while a plugin panel has focus: the editor's own keys bind to the
editor surface, so typing inside a plugin's terminal or form does not save or navigate your
document.

## Viewing and changing bindings

Press `Ctrl+K`, click the keyboard button in the title bar, or choose **Keyboard shortcuts**
from the application menu. The panel opens over the current window. **Panel shortcuts** shows
actions for the area that had focus when you opened it; **Global shortcuts** shows the available
actions across the application. `Alt+Left` and `Alt+Right` switch tabs while keeping your search.

Search by description, or click the shortcut search button and press the keys you want to find.
Recording keys does not run their actions. Bindings can contain one key combination or two
combinations pressed in order; the second combination must arrive within two seconds. `Esc`
ends recording first; when editing a binding, it cancels the draft without saving. Another `Esc`
closes the panel and restores the previous focus. Clicking the shaded background also closes it.

Click a binding to edit it, or use a row's controls to add another binding, remove one, or restore
the defaults. New letter and digit combinations require a modifier beyond `Shift`, such as `Ctrl`
or `Alt`. Save applies the change
immediately to all windows and keeps it across restarts and workspaces. Each action still uses
its own focus context. If a binding conflicts, review the affected actions and explicitly replace
the conflicting bindings; their other bindings stay available. Leaving an unsaved edit asks
whether to continue editing or discard it.

Commands from disabled, removed, or unavailable plugins disappear from the list, while their
saved bindings remain. When a command returns, its binding is restored if available. If another
action now owns that key or a conflicting sequence, the retained binding stays inactive and the
row offers **Resolve conflict**. In a restricted workspace, plugin commands stay unavailable.
