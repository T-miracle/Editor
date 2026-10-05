---
title: Native UI
description: The ui.native document tree, canvases, events, dialogs, theming and the limits the host enforces.
section: sdk
order: 2
alternate: /zh-cn/sdk/ui/
---

# Native UI protocol v1

## Composable layout (manifest protocol 7)

`ui.collections ^1` adds `Kind::SideTabs` and `Document.menu`. SideTabs is an ordinary layout
node that can be composed anywhere in a row/column tree; the node ID equals the collection
ID, and entry actions are still reported by stable ID. A plugin responds to `Resize(width)`
through the node's width, and an adjacent canvas receives its own measured size. A PopupMenu
anchor is relative to the document, covers the body and takes no layout space; while it is
shown, only a selection or dismissal of that menu is accepted, and a Dialog takes priority.

`ui.canvas >= 1.1` adds `Canvas.font` and `Canvas.scroll: Option<ScrollRange>`. The font
overrides the canvas's inherited value and takes part in grid measurement; `content` and
`offset` of the range use logical pixels, dragging the native scrollbar produces
`CanvasEvent::Scroll { offset }`, and the host keeps no second copy of terminal content.
Row wheel scrolling of a character grid converts through `GridMetrics.cell_height`, while an
ordinary canvas keeps pixel events.

A document revision identifies the interaction target and its semantics; it is not a
drawing-frame counter. Input targets, session or process identity and modal changes should
increment it, while ordinary output, drawing and key feedback keep the version so that a Key
or Text event in the same frame, or input still in flight, is not wrongly judged stale. A
follow-up operation from an asynchronous plugin call binds to the stable identity of its
original target and is dropped once that target is revoked; it never follows current focus.

A new plugin negotiates `ui.native ^1` and returns a `ui::Document` through
`api::Output.views`. `Column`, `Row`, `Scroll` and `Tabs` nest standard components and
`Kind::Canvas` at any position, with no terminal-specific slot. A canvas additionally needs
`ui.canvas ^1`, and a character grid appears only when `ui.grid ^1` was negotiated and
`Canvas.grid = true` is set. A UI capability grants no file, document or process permission.

`Canvas.paint` uses the generic `Paint::Fill/Text/Svg`. SVG is drawn by a background
restricted renderer that is denied filesystem and network reads from its environment. Each
document holds at most 2048 nodes, 32,000 paint operations and 16 SVGs, and at most 2 MiB
after serialization. Invalid layouts are rejected before publication. Native text uses the
host font; a plugin updates its own drawing colours and font size through
`Notification::Theme`, and size changes arrive as `CanvasEvent::Resize` on the canvas node.

Every `UiEvent` carries the current document revision and the node ID. The host first checks
the active node, the modal scope and the event type; a stale version returns
`StaleRevision`, a missing or disabled node returns `InvalidHandle`, and a mismatched event
type returns `InvalidRequest`. These rejected host callbacks do not crash the plugin. Stable
node IDs preserve input focus and composition. An interactive canvas must set
`focusable=true` explicitly; uncommitted IME text stays in the host and is delivered to that
canvas as `CanvasEvent::Text` after commit, while an adjacent input handles its own input
independently. Mouse buttons are numbered 0 left, 1 middle, 2 right, and coordinates are
relative to the canvas node.

An editor preview declares `position:"editor"` and `file_extensions`, uses a workspace
instance, requests `editor.read` and negotiates `editor.documents ^1`. The host sends
`Notification::Preview { document, text }`, where `DocumentVersion` points at the in-memory
document and `text` includes unsaved modifications. A plugin must return the version
unchanged in `ui::Document.source`; a stale preview never replaces a newer document.
`document:None` with empty text cancels the current preview. An ordinary panel leaves
`source` empty. Hiding or unloading destroys the native event target and removes its layout
space.

An independent SDK example lives in `plugins/capability-example/src/composition.rs` with
`composed-ui.json` beside it; configuring the example's `label` as `composable-ui` shows the
composed interface, and the `ui-layout` command takes `form`, `canvas` or `combined`.

## Common interface elements

| Kind / constructor | Purpose | Events |
| --- | --- | --- |
| `Column` / `Node::column` | Vertical automatic layout | — |
| `Row` / `Node::row` | Horizontal automatic layout | — |
| `Scroll` / `Node::scroll` | Vertical scroll area with a native scrollbar; set height to constrain the viewport | — |
| `Text` / `Node::text` | Plain text, wrapped by its container | — |
| `Button` / `Node::button` | Native button, keyboard activatable | `Click` |
| `Input` / `Node::input` | Single-line native editing with Chinese IME, selection, undo and clipboard | `Change(String)`, `Submit(String)` |
| `Checkbox` / `Node::checkbox` | Checkbox | `Toggle(bool)` |
| `Choice` | Single-choice group with stable option IDs; options can be disabled | `Select(option_id)` |
| `Tabs` | Tab strip with the current page's content | `Select(tab_id)` |
| `List` | Read-only text list | — |
| `Table` | Read-only text table | — |
| `Separator` | Separator line | — |
| `Progress` | Determinate 0–100 progress bar with an accessibility label | — |
| `Spacer` | Placeholder space with width, height or grow | — |
| `Document.dialog` | Modal overlay reusing main-program native controls | `Dismiss` |

`Node::new(id, Kind::...)` constructs every type. The `gap`, `padding`, `width`, `height`,
`grow`, `role` and `disabled` constructors correspond to the public `layout`, `role` and
`disabled` fields. Layout sizes are logical pixels. `disabled` applies to the whole subtree;
nodes behind a hidden tab or a modal dialog receive no action events.

## Identity, state and events

- A node ID is unique and stable within one panel document, including all tabs and dialogs,
  and allows ASCII letters, digits, dots, underscores and hyphens. The main program preserves
  input state and scroll position by ID; do not generate random IDs every frame.
- Each plugin response may submit one complete document, and the main program reconciles
  native state by ID. `revision` is maintained by the plugin and returned with events, so the
  plugin can recognise actions produced by an older interface; events are not GPUI callbacks
  or memory pointers.
- The outer event is
  `api::Input::Event { panel: Some(panel_id), event: api::Notification::Ui(...) }`.
  `UiEvent.node` is the node ID and `action` is the typed event. Choices and tabs return
  stable IDs, never array indices.
- An input's `value` is its initial value. While `value_revision` is unchanged the main
  program preserves the in-progress draft, so a slower plugin reply cannot overwrite newer
  input. A plugin only needs to update its own state when responding to `Change`; clearing,
  loading or resetting the field must increase `value_revision` to replace the native draft.
  A programmatic replacement does not raise `Change`.
- Button and choice state is owned by the plugin: return the new `checked` or `selected` after
  receiving an event. When a plugin is unloaded, replaced or removes a node, its native input
  and subscriptions are released with the view.

## Dialogs

```rust
use plugin_protocol::ui::{Dialog, Document, Node};
let document = Document::new(Node::button("open", "Open"))
    .dialog(Dialog::new("settings-dialog", "Plugin settings",
        Node::column("settings-body", vec![
            Node::text("hint", "Dialogs use the same node tree"),
            Node::button("save", "Save"),
        ]).gap(8.)
    ));
```

Clicking close or pressing Escape returns `Dismiss` with the dialog ID as `node`; the plugin
should set the next document's `dialog` to `None`. The meaning of a confirm button is defined
by the plugin. The modal stays open while a reply is awaited, and repeated close requests are
sent once. Focus from before the dialog is restored when it closes. Protocol v1 allows at
most one modal overlay per panel; arbitrary system-level windows and nested dialogs are not
provided.

## Theming and fonts

Nodes declare style roles only; there are no hard-coded colour or font fields. When `role` is
omitted, the default is `button`, `input`, `checkbox`, `choice`, `tabs`, `text`, `list`,
`table`, `progress`, `separator`, `scroll`, `spacer` or `container` as appropriate, and a
dialog shell uses `dialog`.

Themes are configured under `themes[].plugins[plugin ID]`, for example:

```json
{
  "ui": {
    "button": {
      "background": "#243044",
      "foreground": "#FFFFFF",
      "hover_background": "#33455F",
      "active_background": "#405777",
      "border": "#62738A"
    },
    "choice": { "accent": "#66B3FF", "accent_foreground": "#101820" }
  },
  "typography": {
    "button": { "family": "Segoe UI", "size_px": 14, "bold": true }
  }
}
```

Each role accepts whichever of
`background/foreground/border/hover_background/active_background/accent/accent_foreground`
apply to that control. Anything undeclared inherits the editor's current theme, and font
roles inherit the theme UI font. When the main program switches theme it repaints the
existing tree directly and continues to send `Notification::Theme` to plugins.

## Limits and compatibility

- `Document.version` must currently be 1; the constructor sets it.
- One document holds at most 2048 nodes including expanded choices, list rows and table
  cells, with a nesting depth of at most 24. A single text run is at most 64 KiB and all text
  together at most 1 MiB.
- Width and height must be finite values between 0 and 10000; gaps and padding are 0–256; a
  dialog width is 240–1200; a table has 1–32 columns with a consistent column count per row.
- Duplicate IDs, invalid selected values and unknown protocols are rejected. All documents of
  one Output are validated first and published together, so a partial interface update cannot
  occur.
- Canvas, SideTabs and standard components share one node tree; arbitrary GPUI objects, rich
  text editors and virtualized tables are not provided. The old Scene, Widget, controls and
  chrome transport has been removed, and old packages are rejected explicitly before install
  and recovery.

Run `Document::validate()` to verify a protocol at runtime; the SDK ships contract tests
beside it. The main program additionally has native regression tests for clicks, input,
dialogs and real WASM packages.
