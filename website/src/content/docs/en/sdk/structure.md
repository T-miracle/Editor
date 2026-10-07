---
title: Document structure
description: Independent readonly outline trees, precise navigation, package artwork and structural folds.
section: sdk
order: 7
alternate: /zh-cn/sdk/structure/
---

# Document structure

`language.structure: ^1` contributes an independent document structure provider. It requires
a WASM `component`, `editor.read` consent and a trusted workspace, but no language server,
process declaration or service permission. Recognition determines the document's language;
the host selects `structure:<language>` separately from highlighting, LSP and formatting.
Existing valid selections are retained when another package is installed. User/project
provider settings and candidate removal use the normal provider rules.

```json
{
  "protocol": 7,
  "component": "plugin.wasm",
  "api": {"required": {"language.structure": "^1"}},
  "permissions": ["editor.read"],
  "structure_providers": [{"id": "structure", "language": "novel"}]
}
```

## Pure snapshot callback

Handle `api::Notification::LanguageStructure(structure::Request)` by returning
`api::Output.language_structure: Some(structure::Proposal)`. A request contains a nonzero
request nonce, the provider ID, resolved settings and `language::SourceSnapshot`. The source
includes the open document's ID, workspace-relative path, revision and at most 1 MiB of UTF-8.
Copy the request nonce and document version into the reply. A document version identifies
an editor incarnation, not merely a filename.

The host runs this callback in an independent, fuel/deadline bounded pure instance. It restores
no private snapshot, inherits no environment or filesystem preopen, and denies every host
operation, including reading assets, editing, file access and process/UI creation. Settings
are readonly input, not permission grants. Initialization and other callbacks cannot publish
structure results. Return only the typed structure field; mixed callback outputs are rejected.

## Nodes and folds

`Proposal.nodes` is a complete tree of `structure::Node` values. Each node has a display
`name`, an opaque plugin-defined `kind`, optional theme artwork, a `range` covering the
definition and its descendants, a precise `definition` destination, and ordered `children`.
All ranges are half-open UTF-8 byte offsets into exactly the supplied text. The definition
must be nonempty and contained by coverage; children must be contained by their parent,
and siblings cannot overlap. There is no host definition-type whitelist.

`Proposal.folds` contains independent complete structural ranges. A comment may be foldable
without appearing in the outline. Fold ranges may nest but cannot cross; the host converts
multiline ranges to the native editor's line-fold mechanism. Do not use coverage as a
navigation or folding substitute. Replies allow at most 4,096 nodes, 128 tree levels,
4,096 folds and 512 KiB of serialized structure data. Names allow 512 bytes and kinds 100;
both must be nonempty and contain no control characters. Invalid data rejects the whole reply.

## Package artwork

`Node.icon` is `{ "light": "icons/definition.svg", "dark": "icons/definition-dark.svg" }`;
`dark` is optional. References are bounded relative paths inside this exact installed package
version. Absolute paths, URLs, traversal and cross-package access are refused. The runtime
canonicalizes the owning package and asset path, bounds each read to 64 KiB, and accepts only
geometry-only SVG without DTDs, scripts, text, external links or URL paints. At most 64 distinct
artwork paths are loaded for a reply. Missing files or rejected SVG bytes use the host's default
definition icon, while valid nodes remain navigable. Unsafe path declarations reject the reply.

## Native outline and lifetime

The host renders disclosure, type icon and name in its native tree, and owns selection,
keyboard navigation, theme, focus and scrolling. The outline follows the active editor
document even while the panel has focus. Navigation validates the current snapshot, unfolds
the destination, moves to its definition and focuses the editor. Cursor ownership uses the
innermost coverage range. The optional follow setting controls automatic expansion/scrolling.

Document changes, close/reopen, provider replacement and trust/workspace revocation invalidate
old results. Dropping a host job cancels delivery; bounded pure work may finish but cannot
produce side effects. Retirement marks a lease inactive before releasing its worker memory.
The host checks both the provider instance and current document version before applying data.
When no provider is available, the host displays the native empty state instead of retaining
another document's tree.
A trapped or failed callback retires only its pure lease; malformed proposal ranges reject
the current reply without poisoning later valid calls. Publication does not silently restart
a failed worker. Explicit plugin recovery/re-enabling or configuration replacement may create
a new lease, while the owner's other language roles remain independent.

The outline is a host dock panel, not plugin-owned UI. DockArea owns its live split tree,
dragging, resizing and workspace-local restoration. Plugins must not create an outline panel
or persist a competing layout or mutable document copy.
