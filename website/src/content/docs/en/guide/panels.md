---
title: Panels and layout
description: The explorer, the editor area, plugin panels, and what the status bar reports.
section: guide
order: 4
alternate: /zh-cn/guide/panels/
---

# Panels and layout

The window is a dock: the file explorer and the editor are panes in it, and plugin panels dock
alongside them. Panes can be resized by dragging the divider between them, and the explorer can
be hidden when you want the full width for code.

The title bar carries the window controls and the plugin-loading badge. The status bar at the
bottom reports the state of the active document, including the cursor position and the number
of problems reported for it.

## Plugin panels

A plugin that declares a panel gets one in the dock, on the side its manifest asks for. Panel
content is drawn by the editor from a document tree the plugin publishes, so plugin panels
follow the same theme and behave like the editor's own controls: they take focus, they handle
Chinese input methods, and they respond to the keyboard.

A panel can be hidden and shown again by the plugin itself. When a plugin is disabled or
unloaded, its panels and their resources are removed rather than left as empty frames.

## Where the keyboard goes

Focus decides which surface receives a keystroke. When a plugin panel has focus, the editor's
own shortcuts do not fire: keys go to the panel, so typing in a terminal or a form does not
save, fold or navigate your document. Clicking back into the editor returns focus, and `Esc`
closes the current popup or cancels the interaction in the focused surface.
