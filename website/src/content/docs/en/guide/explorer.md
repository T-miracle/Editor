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

Nothing outside the workspace root is read: parent directories, global ignore files and
symbolic links are not followed. That keeps the tree predictable and keeps the editor from
walking into a linked directory outside your project.

## Toolbar

| Button | Action |
| --- | --- |
| Reveal active file | Expands the tree to the file you are editing and selects it |
| Collapse all | Closes every expanded directory |
| Expand all | Opens every directory in the tree |

Refreshing the tree from disk is `Ctrl+Shift+R`, which is useful after a build, a branch
switch or any other change made outside the editor.

## Opening files

Selecting a file opens it in a tab. Opening the same file again focuses the existing tab
rather than creating a second one, so unsaved edits are never split across two views of the
same file.

A tab with unsaved changes is marked, and closing it asks before discarding them.
