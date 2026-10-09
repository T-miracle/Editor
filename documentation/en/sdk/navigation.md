# Native links and versioned navigation

[简体中文](../../zh-cn/sdk/navigation.md)

ui.links ^1 and editor.navigation ^1 are independent public capabilities. The base protocol stays 7 and UI document version 1. Neither grants text editing, network downloading or process execution. Package versions remain separate.

## User link events

Document.link_events=true requires ui.links. Native RichText uses Base link hit testing and selection; actual user activation sends Action::Link { uri }. Right-clicking or dragging a selection never navigates. Parsing, layout, theme changes and image loading generate no link event. Undeclared links remain inert. Events still validate node, UI revision, modal ownership, enabled state and active instance. URIs hold at most 4096 bytes and no control characters.

An event expresses intent. The plugin validates that the URI belongs to its current parse result and chooses domain behavior; the host does not interpret Markdown, heading slugs, extensions or plugin identity. Raw HTML gains no execution or implicit file/browser access.

Node.links: Vec<LinkTarget { uri, label }> declares native keyboard targets for read-only RichText, Image or image-fallback Text and requires ui.links. A RichText URI is the actual rendered href, not a second decoding. After validation the plugin navigates using the original parsed target. Images/fallback text allow one outer link; RichText may preserve several in occurrence order. Labels hold at most 256 UTF-8 bytes; an empty label receives a localized Open link label. Each target counts toward the 2048-node budget and URI/label count toward the shared text budget.

link_events defaults false even with declared targets. When enabled, the image content can be clicked and activated with Tab, Enter or Space. Text retains native selection and pointer targeting; Tab focus on a target reveals its link button and focus indication. Hidden keyboard targets reject mouse activation. Removing a target releases focus; scene replacement cannot reuse an old pressed gesture, and image reflow cannot turn release into a new link click.

## Navigation requests

EditorOperation::NavigateDocument { document, target } requires editor.navigation, editor.read and an active workspace scope. document is the actual initiating document identity, workspace-relative path and revision; it must still be active before the effect. The ordinary editor queue, cancellation, timeout and side-effect gate apply. Navigation does not write text, selection or Undo.

| NavigationTarget | Authority and effect | Success |
| --- | --- | --- |
| PreviewNode { panel, node, ui_revision } | Owned declared editor panel, visible preview, exact source/UI revision, active source range and latest native Scroll ownership; locate at next actual layout. Source/scene replacement clears the wait. | Unit |
| RelativeDocument { path } | Also workspace.read; a URI relative to the source file's parent, physically canonicalized to an existing file inside the owned workspace; uses ordinary document opening. | Opened { document }, the actual target identity and revision |
| ExternalUrl { url } | Also navigation.external; HTTP(S) only, with host and no username/password; opens the system browser. | Unit |

Preview panel/node IDs hold at most 100/128 bytes and belong to the instance. Node.source_range must lie within actual current UTF-8 text boundaries. No active scroll ownership, a hidden pane, modal obstruction or an unpainted preview fails rather than navigating another panel or document.

Relative path holds at most 4096 bytes without query/fragment; the plugin separately handles domain fragments. Public document_relative_path/decode_uri_component decode percent-encoding exactly once as UTF-8 and preserve literal +; the request retains the original URI. Parent segments are allowed but the final physical target cannot escape the workspace. Absolute paths, backslashes, drive prefixes, alternate streams, device names, controls and Windows-invalid filename characters are rejected. Links/junctions undergo canonical boundary checks. a%23b.md denotes a literal # filename; %252e is not decoded again. Missing/unreadable targets never open a different document.

Cross-file anchors wait for the actual Opened receipt and exact target preview before requesting PreviewNode. Do not guess identity, retain an old revision or locate a third document. Cancellation, switching, close, disable, permission revocation and instance retirement follow the existing request lifecycle. Accepted does not mean effect completion.

Invalid targets return InvalidPath/InvalidState, missing resources NotFound, stale documents/scenes StaleRevision, missing capability/permission CapabilityUnavailable/PermissionDenied, and cancellation/timeout Cancelled/TimedOut. Other schemes never become shell commands; browser opening grants no process.exec and downloads no URL contents.
