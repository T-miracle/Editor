# Plugin SDK — viewport

公开契约正文统一维护在双语 Markdown 文档，SDK 导出包含同一份英文正本。

- [English](../../documentation/en/sdk/viewport.md)
- [简体中文](../../documentation/zh-cn/sdk/viewport.md)

本目录保留版本化 Rust 类型、WIT 与 JSON 方法结构，供宿主和独立访客使用；本文仅作为迁移入口。

Set `Document.editor_viewport` to the ID of one active root `Scroll`. `Document.source` must echo the exact authorized Preview input. An owned workspace editor panel must negotiate `editor.viewport`, `ui.richtext` and have `editor.read`. Ordinary panels default to no binding. Nodes use existing bounded `SourceRange` UTF-8 mappings; the host validates actual source boundaries before locating.

The plugin owns its synchronization control, default and workspace/file-type preference through `ui.tools` and `storage.private`. It publishes `editor_viewport` only while its current layout and intent request synchronization. The host owns no chain control or fixed display modes. Withdrawing the binding, hiding a side, changing source, opening a modal, retiring the instance or removing the contribution cancels pending measurements and locations; retained readonly paint cannot execute an old reveal.

## Notifications and requests

`Notification::SourceViewport(SourceViewport)` contains the current document/UI revision, top visual caret offset, the fraction of its visual row above the viewport, optional origin and a layout flag. `Action::Viewport(PreviewViewport)` is emitted on the declared Scroll and identifies a measured source block, its exact source range and the fraction above the viewport. These notifications coalesce to at most one per completed native layout. Fractions are finite and bounded to `[0,1]`; source offsets/ranges remain at most 1 MiB. Block IDs are at most 128 UTF-8 bytes.

`EditorOperation::LocateViewport { document, panel, ui_revision, target, origin }` uses `ViewportTarget::Source { offset, line_fraction }` or `Preview { node, fraction }`. Nonzero `origin` identifies a programmatic request. The regular scoped editor request lifecycle handles cancellation, deadlines and queue quotas. A Unit result acknowledges a native locate queued under current authority; every subsequent layout checks source/UI identity, split mode and preference again. If geometry has been revoked before it paints, it is discarded rather than applied to a new scene.

Native programmatic movement echoes `origin: Some(...)`. Manual input has no origin. Guests must not turn programmatic receipts into reverse locations. The `layout` flag distinguishes image/width/wrapping reflow from manual vertical scrolling; guests can preserve the last manual driver and refresh its following side. Do not republish a UI document merely to acknowledge a viewport event. Retain at most one pending request and one latest position when coalescing fast input.

Source positioning uses public native Base layout and bounded seeks (at most 48 layout steps); it does not assume buffer rows equal wrapped visual rows. Unavailable/folded geometry can fail without implicitly unfolding text. Preview positioning uses current measured block height and the nearest Base scroll owner, so an image or table does not imply an article-wide percentage mapping. End-of-document locations clamp to the real available extent.

## Failure and lifecycle

Unnegotiated capability yields CapabilityUnavailable; wrong scope/grant/owner yields PermissionDenied; invalid origin, fraction, target or UTF-8 boundary yields InvalidRequest. Closed, switched or edited sources and replaced UI scenes reject StaleRevision. Hidden or disabled split synchronization rejects InvalidState. A locate arriving during a native pointer gesture is Cancelled, including motion outside the pane. Neither a successful queued acknowledgement nor cancellation claims that later manual scrolling will be overridden.

Manual wheel, keyboard and pointer input retains ownership after its event is sent and after pointer release. A delayed reverse locate returns Cancelled until real input on the other side or explicit preview navigation transfers ownership. Sending a notification or acknowledging a programmatic request never transfers ownership. This also protects a small wheel notch whose source translation is smaller than one whole text row.

The distributed SDK includes the request/notification types and this document. Independent guests use the same public contracts as bundled previews.
