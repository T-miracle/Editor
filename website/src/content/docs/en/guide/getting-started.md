---
title: Getting started
description: Open a file or a project folder and know what the editor does with each.
section: guide
order: 1
alternate: /zh-cn/guide/getting-started/
---

# Getting started

Nanobug is a native desktop application and does not require an account.

## Installing and updating

On Windows, run the Nanobug x64 Setup installer and follow its English or Simplified Chinese
wizard. It installs for your user account, adds a Start menu entry, and offers a desktop shortcut.
The installed application and plugin files do not require Rust or Cargo.

Close all Nanobug windows before updating or uninstalling. Run a newer installer to update the
same installation. Uninstall through Windows Installed apps; settings, history, and installed
plugin data in the existing `MeEditor` user-data directories are retained.

macOS packaging uses an application bundle in a DMG (drag Nanobug to Applications), or a PKG
installer. Linux packaging uses DEB or RPM packages installed with the distribution's package
manager. These packaging paths are provided for compatibility and have not been built or
installation-tested in the current phase. macOS signing/notarization and platform-specific
plugin dependencies require separate release preparation.

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
