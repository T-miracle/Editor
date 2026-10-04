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

`Ctrl+S` writes the buffer to disk. Saving is what triggers the local history snapshot; the
snapshot lives in the user data directory, not in your project.

If the file changed on disk since it was read, the editor tells you instead of overwriting
silently, and keeps your unsaved edits. Saving again resolves the conflict.

## Shortcuts

| Key | Action |
| --- | --- |
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
