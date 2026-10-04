# Semantic editor viewports — editor.viewport 1.0

This additive capability binds a native source editor to a source-mapped preview. It never changes text, selection, focus, IME, folding or Undo, and creates no second document or scroll handle.

## Declaration and authority

Set `Document.editor_viewport` to the ID of one active root `Scroll`. `Document.source` must echo the exact authorized Preview input. An owned workspace editor panel must negotiate `editor.viewport`, `ui.richtext` and have `editor.read`. Ordinary panels default to no binding. Nodes use existing bounded `SourceRange` UTF-8 mappings; the host validates actual source boundaries before locating.

The host presents a local chain control beside optional editor presentation buttons; a panel without `view_modes` still gets the chain and defaults to split. Synchronization defaults on, persists per workspace/contribution, and operates only in split mode. Turning it off, hiding a side, changing source, opening a modal, retiring the instance or removing the contribution withdraws pending measurements and locations.

## Notifications and requests

`Notification::SourceViewport(SourceViewport)` contains the current document/UI revision, top visual caret offset, the fraction of its visual row above the viewport, optional origin and a layout flag. `Action::Viewport(PreviewViewport)` is emitted on the declared Scroll and identifies a measured source block, its exact source range and the fraction above the viewport. These notifications coalesce to at most one per completed native layout. Fractions are finite and bounded to `[0,1]`; source offsets/ranges remain at most 1 MiB. Block IDs are at most 128 UTF-8 bytes.

`EditorOperation::LocateViewport { document, panel, ui_revision, target, origin }` uses `ViewportTarget::Source { offset, line_fraction }` or `Preview { node, fraction }`. Nonzero `origin` identifies a programmatic request. The regular scoped editor request lifecycle handles cancellation, deadlines and queue quotas. A Unit result acknowledges a native locate queued under current authority; every subsequent layout checks source/UI identity, split mode and preference again. If geometry has been revoked before it paints, it is discarded rather than applied to a new scene.

Native programmatic movement echoes `origin: Some(...)`. Manual input has no origin. Guests must not turn programmatic receipts into reverse locations. The `layout` flag distinguishes image/width/wrapping reflow from manual vertical scrolling; guests can preserve the last manual driver and refresh its following side. Do not republish a UI document merely to acknowledge a viewport event. Retain at most one pending request and one latest position when coalescing fast input.

Source positioning uses public native Base layout and bounded seeks (at most 48 layout steps); it does not assume buffer rows equal wrapped visual rows. Unavailable/folded geometry can fail without implicitly unfolding text. Preview positioning uses current measured block height and the nearest Base scroll owner, so an image or table does not imply an article-wide percentage mapping. End-of-document locations clamp to the real available extent.

## Failure and lifecycle

Unnegotiated capability yields CapabilityUnavailable; wrong scope/grant/owner yields PermissionDenied; invalid origin, fraction, target or UTF-8 boundary yields InvalidRequest. Closed, switched or edited sources and replaced UI scenes reject StaleRevision. Hidden or disabled split synchronization rejects InvalidState. A locate arriving during a native pointer gesture is Cancelled; manual ownership lasts until release, including motion outside the pane. Neither a successful queued acknowledgement nor cancellation claims that later manual scrolling will be overridden.

The distributed SDK includes the request/notification types and this document. Independent guests use the same public contracts as bundled previews.
