---
title: Getting started
description: Open a file or a project folder and know what the editor does with each.
section: guide
order: 1
alternate: /zh-cn/guide/getting-started/
---

# Getting started

Nanobug is a native desktop application. There is no installer wizard and no account: you
run the executable and it opens a window.

## Opening a file

Pass a file path when you start the editor, or use the explorer to reach one. Opening a file
loads it as UTF-8 and asks the installed plugins which language it is, which decides the
syntax highlighting and whether a language server is available for it.

The window title and the editor status bar report the file name and the language the editor
resolved for it. A dot on the tab means the buffer has unsaved changes; `Ctrl+S` saves them.

## Opening a project folder

Pass a directory instead of a file. The editor treats it as the workspace root and builds the
file tree from it, so `.gitignore` and `.ignore` rules inside that directory decide which
files appear.

A workspace is also the unit that plugins and language services are scoped to. Opening a
folder is therefore what makes language features such as go-to-definition and diagnostics
available for the project; a file opened on its own belongs to no workspace.

## What the editor expects from your project

- Files are read and written as UTF-8. Other encodings are not converted.
- The file tree follows `.gitignore` and `.ignore`; nothing outside the workspace is read.
- Language features need the matching plugin, and the plugin may need a language server that
  you have installed separately. The editor does not download tools on its own.
- Saving writes a history snapshot outside the project directory, so your repository does not
  gain editor files.
