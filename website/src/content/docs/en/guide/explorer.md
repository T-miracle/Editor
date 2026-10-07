---
title: Explorer
description: How files are listed, which ones are hidden, and what the toolbar buttons do.
section: guide
order: 3
alternate: /zh-cn/guide/explorer/
---

# Explorer

The explorer lists the workspace you opened. It is a tree, not a flat list: directories expand
in place, and the tree can be expanded or collapsed wholesale from its toolbar.

## Ordering

Entries are ordered by three rules, in this order:

1. Directories before files.
2. Alphabetical order ignoring capitalisation, so `README.md` and `Cargo.toml` sit where a
   reader expects them rather than being split by case.
3. Chinese names are ordered by pronunciation rather than by code point, so `北京` sorts among
   the `b` entries.

## What is hidden

The tree follows the `.gitignore` and `.ignore` files inside the workspace, including nested
ones and negation rules, with `.ignore` taking precedence. Files that those rules exclude are
not listed.

Tree listing stays inside the workspace root: parent directories and global ignore files are
not read, and symbolic links are not followed. Files you explicitly offer for import can be
read from outside the project.

## Toolbar

| Button | Action |
| --- | --- |
| Reveal active file | Expands the tree to the file you are editing and selects it |
| Collapse all | Closes every expanded directory |
| Expand all | Opens every directory in the tree |

Refreshing the tree from disk is `Ctrl+Shift+R`, which is useful after a build, a branch
switch or any other change made outside the editor.

## Opening files

Clicking selects a row; double-clicking a file opens it in a tab. Double-clicking a directory
or clicking its arrow expands or collapses it. Opening the same file again focuses the existing tab
rather than creating a second one, so unsaved edits are never split across two views of the
same file.

A tab with unsaved changes is marked, and closing it asks before discarding them.

## Importing and moving files

Paste files and directories copied or cut in your system file manager using the explorer's
context menu or `Ctrl+V` while the tree has focus. Copy preserves the source; cut moves it.
An external file drop copies the offered files. Dragging one tree row moves it; hold `Ctrl`
on Windows/Linux or `Option` on macOS to copy instead. Folders target themselves, files target
their parent, and blank space targets the project root. The destination directory is highlighted.

Hover over a closed directory for about 600ms to expand it. Drag near the tree's upper or lower
edge to scroll. Cancelling restores directories temporarily opened during that drag.

When names collide, choose **Skip**, **Keep both**, or **Replace**, optionally for subsequent
conflicts in the batch. Replacing a directory merges its contents and keeps target-only files;
colliding children still require a conflict choice. An unsaved target cannot be replaced during
a normal transfer. Moving an open file keeps its editor content and text undo history.

Long operations show progress and can be cancelled. Completed items remain after cancellation
or failure; the current incomplete result is cleaned up and later items are not started.
Symbolic links and Windows junctions are skipped. Errors and skipped paths remain in a message
until you close it.

## Undo and redo file operations

The context menu names the recorded file operation. With the tree focused, `Ctrl+Z` undoes it
and `Ctrl+Shift+Z` redoes it. In the editor, these shortcuts continue to undo and redo text.
Copies, moves, replacements and directory merges can be recovered within the current session;
backups stay outside your project and are removed when the session closes.

Recovery checks today's disk contents. Later edits or occupied paths require confirmation.
Forcing recovery of unsaved editor content requires a separate confirmation to discard those
edits. Redo also checks for new conflicts. New-file and delete commands are outside this file
transfer history.

Windows system file clipboard and drag input have dedicated support. macOS reuses native file
paths and file-URI clipboard offers; native Finder cut intent is not implemented. Linux file-URI
offers are supported, but native file clipboard support depends on the platform backend.
macOS and Linux have not received the Windows native interaction acceptance run.
