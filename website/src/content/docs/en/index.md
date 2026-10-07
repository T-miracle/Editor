---
title: Nanobug documentation
description: How to use the editor and how to build plugins for it.
section: guide
order: 0
alternate: /zh-cn/
---

# Nanobug documentation

Nanobug is a native desktop code editor written in Rust with GPUI. It is not a web
application: the window, the editor surface and the plugin panels are all drawn natively.

This site has two audiences. Pick the journey that matches what you are doing.

- **[Editor guide](/en/guide/)** — install the editor, open a project, edit and
  navigate code, and configure what the editor does on your machine.
- **[Plugin contract](/en/sdk/)** — build a plugin package: declare capabilities,
  negotiate versions, draw native UI, run services and processes, and package the result.

## What the editor does today

- Adjustable native layout with a file explorer and a code editing area.
- Recursive file tree that follows `.gitignore`.
- Light and dark themes with per-file-type icons.
- Opens UTF-8 files and highlights them by extension.
- Syntax highlighting and diagnostics come from plugins that ship WebAssembly grammars,
  so a language can be added by installing a package rather than editing the editor.
- Go to definition with `F12` or a middle click; errors reported by plugins or by a
  language server are navigable with `F8` and `Shift+F8`.
- Line numbers, indent guides, code folding and soft wrap toggles.
- Move the selected text by dragging it with the left mouse button; the original
  selection is restored by a single undo.
- Save with `Ctrl+S`, with a local history snapshot written outside the project directory.
- Runtime plugin system: install from a local package, confirm permissions, hot-update,
  and dock plugin panels next to the editor.

## Where the rest lives

Source build instructions, the workspace layout and the test commands are for people
working on the editor itself; they stay in the repository README rather than on this
site. Plugin packages document their own usage in the package README, which the editor
shows inside the plugin manager.
