---
title: Native UI
description: The ui.native document tree, canvases, events, dialogs, theming and the limits the host enforces.
section: sdk
order: 2
alternate: /zh-cn/sdk/ui/
---

# Native UI protocol v1

## File layouts and viewers

With `editor.layout ^1`, a selected workspace file provider publishes
`Document.editor_layout = true` and composes the centre using native Row, Column and Scroll
nodes. Text layouts bind to `Document.source`; read-only files bind to `Document.file`.
The contribution needs `editor.read`, a workspace instance and a non-auxiliary editor panel.

`Kind::NativeEditor { document: DocumentVersion }` borrows the existing native editing
session, preserving its text, selection, undo and IME state. At most one reference is
allowed, only in the layout root, and its target must equal `Document.source`. References
in toolbars or dialogs, cross-file references and stale versions are rejected. Omitting
the reference displays only plugin content. A non-text file cannot borrow another file's
editor. Hiding the input target releases focus; restoring it reuses the original session.

`Panel.auxiliary = true` declares an editor contribution for supplementary tools; it
cannot become a centre provider. The host remembers provider identity per workspace and
file type. A sole initial candidate is adopted and remembered; competing candidates
require explicit selection from the file-tab context menu. Installation preserves a valid
choice, and temporary failure preserves the user's preference. Text files fall back to
their ordinary editor; non-text files retain their tab, failure reason and alternative
viewer actions. Switching providers advances target versions, so events from old sources,
UI revisions and instances remain rejected. Plugins own layouts, modes and display
preferences; the host validates ownership and permissions and performs native drawing.

`editor.files ^1` delivers `Notification::FilePreview { file: Option<FileContext> }`.
File versions have independent IDs, workspace-relative paths and monotonic revisions;
`file_type` is the normalized extension. `text` carries a `DocumentVersion` only when the
file actually has text capability. Closing, switching or reopening revokes old resources;
non-text files create no text session. This entry requires a trusted workspace, negotiated
capability, `editor.read` and a declared editor panel in a workspace instance.

`file_extensions` matches text and `readonly_file_extensions` matches read-only files;
their combined limit is 32. The latter needs `editor.files`. Disabled installed declarations
still inform the file model. When a viewer is unavailable, generic content detection
retains the non-text tab and offers an error and retry.

`ui.file_images ^1` adds `Kind::FileImage { alt, sizing }`, bound to the exact current
`Document.file`. It reads only that file; a plugin cannot replace its resource URI.
Background reading needs `workspace.read` and validates canonical paths, instance
ownership and workspace boundaries. A document permits at most 64 image nodes and each
encoded image at most 8 MiB. `OriginalContain` uses
`min(1, available_width/image_width, available_height/image_height)`: preserve aspect
ratio, centre, show the whole image and never enlarge automatically. `Contain` explicitly
allows enlargement. Resource changes, permission revocation and instance retirement
revoke old results.

## Footer tools and preferences

`ui.tools ^1` adds at most 32 `Document.tools` buttons. Each has a stable ID, bilingual
`LocalizedText` name and tooltip, package SVG paths in `ToolIcon.light/dark`,
`visible/selected/disabled/order` and an explicit `ToolTarget`. A File target must equal
`Document.file`; a Window target belongs to that contribution's independent window.
Only independent panels provide window visibility controls. Layout and auxiliary
contributions do not create window buttons.

The host draws window buttons to the left of the short divider and plugin function buttons
to its right. Multiple applicable file contributions can supply tools. Plugins implement
functions through `Notification::Tool(ToolEvent)`; footer and overflow entries share state
and events. Activation preserves the file or window target captured before the click.
Switching, reopening, replacement, modals, hidden or disabled controls, stale UI revisions
and revoked permissions invalidate old targets. Instance retirement removes contributions.

Icons are package-relative geometric SVGs of at most 64 KiB, 512 XML nodes and 16 nested
geometry levels. Allowed elements are `svg/g/path/rect/circle/ellipse/line/polyline/polygon`
with geometry, stroke and colour attributes. Named colours, hex and `currentColor` are
accepted; CSS, URL paint, href, images, text, scripts, DTD and XML stylesheets are rejected.
Publication and drawing both check bytes, canonical boundaries and ownership. Missing,
unsafe or oversized artwork rejects publication, without a host-specific fallback.
The capability grants no process or file access; File tools require `editor.read` and a
workspace instance.

`storage.private >=1.1, <2` adds permission-gated `ReadPreference/WritePreference`.
The SDK exposes `read_preference(key, watch)`, `write_preference(key, expected_revision, data)`
and `PreferenceBinding`. The host supplies plugin/workspace namespaces; keys contain only
normalized file type and plugin-local name. Missing data is revision 0, writes use
compare-and-set, unchanged values do not advance revisions, and stale writes return
`Conflict`. JSON is bounded at 64 KiB and counts toward private storage. Corrupt records
are reported and preserved. Up to 32 revocable watches deliver `PreferenceChanged`;
unreadable records terminate their watch with `SubscriptionFailed`. Plugins bind, validate,
reread after conflicts and close subscriptions themselves. Preferences never store editor
text or undo state and grant no UI or document authority. The distributed SDK's
`TOOLS.md` describes the full binding and finite legacy preference import contract.

## Retired presentation interfaces

`editor.presentation`, `Panel.view_modes`, `PreviewMode/PreviewModes`,
`Installed::preview_mode_icon` and `Command.toolbar/toolbar_icon` have been removed.
Use `editor.layout` with `NativeEditor` for centre content and `ui.tools` for function
buttons. Commands retain menu, shortcut and invocation entry points. Panels and commands
reject unknown fields; retired fields or required capabilities explicitly prompt an SDK
update. Finite import writes legacy intent into authorized plugin private data and never
restores the retired runtime protocol.

## Composable layout (manifest protocol 7)

`ui.native >=1.1, <2` adds `Layout.resizable` for non-wrapping Row/Column nodes with 2–16
children. Plugins choose the split; the host retains native sizing and drag capture and
draws local dividers. `ui.content_colors ^1` allows at most 64 `role.property` RGB defaults
on `Document.content_colors`; keys have at most 128 ASCII letters, digits, dots, underscores
or hyphens, and values are at most `0xffffff`. Resolution is user plugin theme, current
plugin document defaults, then native theme. Defaults never change another plugin.

During same-session parsing the previous read-only tree and geometry may remain visible,
with its controls, menus and modal input paused. Its NativeEditor displays current native
text and preserves focus. Old trees cannot edit, navigate or reuse new file resources.
Fresh snapshots must echo exact source/file versions; switching files, reopening, changing
provider or retiring an instance clears this seam.

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


`ui.native >=1.1,<2` adds `Kind::Textarea(Input)` and `Node::textarea` for native
multiline fields. It uses the same input value, placeholder, reset revision, enabled state
and Change/Submit event validation as a single-line input. Ordinary echoes retain native
focus, selection and IME composition; advance `value_revision` only for an intentional
replacement. Removed controls release their subscriptions. Textareas use the editor's
local theme and editing behavior and do not create a document session or WebView.

| Kind / constructor | Purpose | Events |
| --- | --- | --- |
| `Column` / `Node::column` | Vertical automatic layout | — |
| `Row` / `Node::row` | Horizontal automatic layout | — |
| `Scroll` / `Node::scroll` | Vertical scroll area with a native scrollbar; set height to constrain the viewport | — |
| `Text` / `Node::text` | Plain text, wrapped by its container | — |
| `RichText` / `Node::rich_text` | Restricted, read-only native HTML; requires `ui.richtext` | Optional versioned link events |
| `CodeBlock` / `Node::code_block` | Read-only monospace code; requires `ui.richtext` | — |
| `Image` / `Node::image` | Authorized source-bound image; requires `ui.images` | Optional link events |
| `FileImage` | Read-only current-file image; requires `ui.file_images` | — |
| `NativeEditor` | Reference to the existing text session; requires `editor.layout` | Existing native editing |
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

## Source toolbars, rich text and navigation

`Document.editor_toolbar: Option<Node>` contributes native nodes above the source editor;
the preview body remains in `root`. It requires an exact `Document.source`,
`editor.toolbar ^1`, `editor.read` and an owned workspace editor panel. Missing source
returns `InvalidRequest`, missing capability `CapabilityUnavailable`, and wrong ownership,
scope or permission `PermissionDenied`. Toolbars, root, menus and dialogs share IDs and
budgets. Events retain panel, UI revision, node and action. Source-only layouts retain the
toolbar; preview-only layouts hide it. Revocation removes targets. Modal and disabled
guards remain effective. This capability alone grants no text write authority.

`Node.tooltip` is localized using `Environment.locale` and contributes to the text budget.
Native buttons use it for hover and accessibility text. `Layout.wrap` defaults to false;
wrapping Rows preserve minimum button sizes and compute their height from content.

`ui.richtext ^1` adds restricted, read-only `RichText { html }`, `CodeBlock { text, language }`
and `Node.source_range` without changing UI document version 1. Plugins parse their own
domain content, filtering or escaping unsupported HTML. The host uses native rich text,
without WebView, script or arbitrary CSS. Default href and image callbacks are intercepted:
rendering never opens a browser, reads files, fetches network/data URLs or bypasses
permissions. Images use separately authorized Image nodes; tasks use Checkbox nodes,
disabled when read-only. Labels and empty states follow `Environment.locale`, supplied at
Prepare and refreshed through Theme; missing locale defaults to Simplified Chinese.

CodeBlock keeps raw whitespace and newlines. Its optional language is 1–100 ASCII letters,
digits or `._+-#`; a label alone starts no service. `Document.code_highlighting`, disabled
by default, requires `ui.code_highlighting ^1`, `ui.richtext`, `editor.read` and an exact
source in an owned workspace editor panel. It reuses the selected dynamic WASM provider;
missing, disabled or failed providers fall back to monospace text. Plugins normalize their
own language aliases. The SDK's `CODE_HIGHLIGHTING.md` specifies cancellation and budgets.

Stable block IDs and half-open UTF-8 `source_range = { start, end }` refer to the immutable
memory text version in `Document.source`, not generated HTML offsets. All mapped node
types require `ui.richtext`. Missing source, reversed ranges or end above 1 MiB are invalid.
The host checks current text length and character boundaries before use; stale targets
remain rejected. Mappings create no second text state or write authority.

`Document.editor_viewport` binds an active Scroll ID to source viewport notifications and
`Action::Viewport`, requiring `editor.viewport`, `ui.richtext`, `editor.read` and exact
source. The SDK's `VIEWPORT.md` specifies split synchronization, origin tags and navigation.
`ui.links ^1` enables explicit `Document.link_events` and `Action::Link { uri }`;
`Node.links` supplies image and keyboard targets. Parsing and drawing never open links.
Node, revision, modal, disabled and instance checks still apply. Versioned
`editor.navigation` requests separately validate permissions and targets; see the SDK's
`NAVIGATION.md`.

## Authorized image resources

`ui.images ^1` adds `Kind::Image { source, alt }`, bound to the exact preview
`Document.source` in an owned workspace editor panel. Source URIs are 1–4096 UTF-8 bytes;
alt text and URIs count toward ordinary text budgets. Root, toolbar and dialog share a
64-image limit. Missing source or excess nodes are invalid; source ranges additionally
require `ui.richtext`. The host reads and decodes bounded background resources rather than
sending image bytes through guest JSON.

Local URIs resolve relative to the source directory after one percent decoding. Absolute
paths, backslashes, device names, colons and alternate data streams are rejected. Parent
segments are allowed only when both source directory and canonical image path, including
symlinks and Windows junctions, remain in the instance workspace. Reading requires granted
`workspace.read`. Remote images accept only credential-free HTTP(S), case-insensitively,
with granted `network.images`; no redirects, environment proxies, auth or cookies are used.
`file:`, `data:` and other schemes are rejected.

Each encoded image is at most 8 MiB, manager residency at most 64 MiB and the process uses
at most eight workers across managers. Queueing and IO share a 30-second deadline.
Late results never revive a terminal timeout. DNS that cannot be interrupted remains in
its counted worker; expiry prevents later connection and creates no extra resolver thread.
Permission, boundary, HTTP, quota or decode failures display the node's alt and localized
reason, preserving other content. Structural and negotiation errors still reject publication.

The public manager exposes immutable `ImageResource { source, uri, state }` snapshots keyed
by plugin/panel/node, with Loading, Ready bounded bytes or Failed states. Native decoding
accepts PNG, JPEG, GIF, WebP and restricted SVG; no external SVG files, network or scripts
run. Decoded pixels share a 64 MiB native cache and a 4096-pixel axis limit. UTF-8 SVG rejects
SVGZ and DTD and bounds XML, depth, expansion, paint reuse, temporary canvas and effect work
before rendering. Quota failures are retained until capacity increases. Active projections
share atlas entries, released when their final projection retires.

Jobs bind instance incarnation, panel, source identity/revision, node and URI. Replacement,
preview revocation, edits, close, disable, uninstall, update and workspace switch disconnect
old consumers without waiting for IO. Snapshots create no mutable source, undo or disk cache.

## Native image input

`editor.images ^1` and `Document.editor_image_input = true` opt the owned source editor
into native paste/drop. They require `editor.read/editor.write`; clipboard capture also
needs `clipboard`, and saving requires `workspace.write`. Guests cannot poll the clipboard
or read external paths. The host validates actual formats and exact document, selection
and epoch before delivering ImageInput metadata; encoded bytes stay in host resources.
Up to eight images, 8 MiB each, 32 MiB per batch and 64 MiB pending bytes are allowed.
Handles expire after 30 seconds and revoke on source/lifecycle changes.

`SaveImageInput { input, name }` creates only a sibling attachment using atomic create-new.
Names are safe single basenames up to 255 bytes with the actual format extension; paths,
device names, ADS and trailing spaces/dots are rejected. Existing files return Conflict
without consuming the handle, allowing a renamed retry. Only successful, matching receipts
consume an input. Cancellation after file IO starts stops waiting and does not roll back
external effects; late success cannot overwrite a cancelled terminal result.

Saving files and editing references are separate operations. Plugins wait for receipts and
use one versioned `editor.edit` transaction for references. Stale targets reject insertion
while completed files remain; text Undo removes references only. The host cleans only
incomplete files created by that request. This grants sibling attachment writes, not
arbitrary workspace file writes.

## Event identity and state

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
- Canvas, SideTabs, read-only rich text and standard components share one node tree; arbitrary GPUI objects, rich
  text editors and virtualized tables are not provided. The old Scene, Widget, controls and
  chrome transport has been removed, and old packages are rejected explicitly before install
  and recovery.

Run `Document::validate()` to verify a protocol at runtime; the SDK ships contract tests
beside it. The main program additionally has native regression tests for clicks, input,
dialogs and real WASM packages.
