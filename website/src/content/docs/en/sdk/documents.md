---
title: Documents and readonly resources
description: Read current sessions, observe changes, and compare provider-owned text.
section: sdk
order: 27
alternate: /zh-cn/sdk/documents/
---

# Documents and readonly resources

`editor.documents` 1.1.0, `editor.virtual` 1.0.0 and `editor.diff` 1.0.0 are stable core
capabilities and are not deprecated. Their compatibility, deprecation and negotiation follow
the [SDK stability policy](/en/sdk/#stability-and-capability-compatibility). The additions on
this page require a host supporting `protocol = 7`, `api.base = ^1` and the respective
capability ranges below. Declare each needed addition as required, or declare it optional
and explicitly disable or replace that feature when it is absent; an operation cannot be
called without negotiation. No earlier wire protocol is restored by these additions.

Negotiate `editor.documents ^1.1` with `editor.read` to enumerate or read open text sessions. `guest::list_documents` and `guest::read_document` return an `EditorTask`; consume its matching `Notification::Request` and use the completed `EditorValue`. Documents have a fresh ID after close/reopen, a normalized local identity or an instance-owned virtual handle, and a revision. A resource identity grants no filesystem or cross-instance authority.

`ListDocuments` returns at most 128 readable sessions plus the active version, including background and unsaved documents. Each `DocumentInfo` reports read/edit/save abilities, dirty state, title, language, encoding, EOL, byte length, selection and laid-out visible rows. Virtual documents from other instances are excluded; their active identity is reported as absent. Local identity paths are plain workspace-relative paths, not percent-encoded URLs. `#` and `%` remain filename characters.

`ReadDocument { document, range }` validates ID, path and revision together before taking immutable text and metadata from the native editor. It never rereads disk. A missing range requests the full text; each response is limited to 256 KiB. Read a large document in bounded ranges at the same version. Any edit, rename, refresh or close invalidates old versions with `StaleRevision`. Revoked/foreign virtual references fail with `InvalidHandle` or `PermissionDenied`; they never resolve through a guessed path.

## Coordinates

`DocumentRange::Bytes` uses half-open UTF-8 byte offsets. Both endpoints must be character boundaries. `DocumentRange::Utf16` and `TextPosition` use zero-based source lines and UTF-16 columns. CRLF is one break, CR and LF also delimit lines, and a surrogate pair cannot be split. Out-of-range coordinates fail with `InvalidRequest`; nothing is clamped. A UTF-8 BOM, if present, remains in the text and coordinates. Visible rows describe native layout (including wrapping/folding), not source line numbers.

For `中😀\r\nsecond`, UTF-16 line 0 columns 1..3 resolve to bytes 3..7; line 1 column 0 is byte 9. Column 2 splits the emoji and is invalid.

## Events and release

`guest::subscribe_document_events()` explicitly opts into `Notification::DocumentEvent`. Its resource handle shares an eight-subscription limit with the existing `SubscribeDocuments` stream and is released through `guest::close_resource`. The older stream retains its local-path `DocumentChange` format and coalesced latest revisions; negotiating 1.1 never silently changes its notification variants. Legacy selection and preview operations do not expose virtual content.

Rich events observe opened, content-changed, closed, will-save, did-save (including failure), active, selection and viewport transitions. Sequence numbers increase within a host workspace session. Native observers may merge intermediate selection/viewport measurements; the FIFO retains all published observations and save/close order. `WillSave` is observational: delivery may occur after the write and cannot block, cancel or intercept saving. Metadata contains no text delta; reread only at an exact event revision and ignore older versions.

Ingress and each subscription queue hold at most 128 events. Overflow clears the incomplete queue, emits terminal `SubscriptionFailed(LimitExceeded)`, and releases the subscription. Enumerate current state and create a new subscription to recover; never interpret a partial stream as complete. Cancelled subscriptions stop delivery even when released inside their own callback. Trust loss, workspace teardown and instance retirement revoke their authority.

## Readonly provider content

Negotiate `editor.virtual ^1` and `editor.read`. `OpenVirtualDocument { title, language, text }` creates an ordinary readonly native Tab, returns `DocumentOpened(DocumentInfo)`, and has no temporary backing file. An instance owns at most 32 resources, each at most 1 MiB. `RefreshVirtualDocument` replaces only an owned exact version; it advances revision and resets readonly selection/undo without creating a user edit. The language is a display hint; a readonly resource starts no native language service.

`OpenDocument { resource }` activates an existing virtual resource or opens a local text file. Local opens additionally require `workspace.read` and canonical workspace containment. `LocateDocument { document, position }` focuses and selects a strict UTF-16 position in either resource.

Virtual content has `edit=false` and `save=false`. Typing, paste and ordinary saves are rejected; readonly sessions cannot write through a file store. Virtual Tabs never enter disk watching, local history or persisted file restoration. `CloseResource`, native Tab close, plugin disable/cutover and window teardown revoke the same authority; the host removes retained native views. Failed candidate preparation leaves a live previous instance intact. A successful replacement gets fresh handles, never revives serialized resource IDs.

## Comparison and examples

Negotiate `editor.diff ^1` and `editor.read`, then send `CompareDocuments { left, right }` with two exact versions. The native comparison borrows the existing EditorState entities, marks removed/inserted/changed line ranges, and makes the left pane readonly. Each pane scrolls independently. Left-focused Save cannot route to the active right file; the right local pane keeps normal editing and saving. Editing/closing either source, changing the active right target or retiring the initiating instance or its delegated caller removes the comparison and only its own decoration layers. This ownership applies when both sources are local files. Request completion does not end the view's lifetime. Closing the comparison returns keyboard focus to the right document.

Comparison accepts at most 1 MiB and 2000 lines per side, with at most 4M LCS cells; larger requests return `LimitExceeded`. It does not create another mutable document, tool page or writable virtual filesystem.

To present historical text or a generated proposal, enumerate sessions, read the chosen exact version with `guest::read_document`, derive immutable content, and open it with `OpenVirtualDocument`. Use the returned version and the source version for `CompareDocuments`. For a refresh, send `RefreshVirtualDocument` with the current owned version, then compare its returned new version with a freshly read source version. If either source changes, read it again and explicitly request a new comparison; an earlier comparison does not track subsequent edits.
