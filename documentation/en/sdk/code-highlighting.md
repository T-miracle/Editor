# Read-only native code highlighting

[简体中文](../../zh-cn/sdk/code-highlighting.md)

ui.code_highlighting ^1 is an independent declarative capability, provided as 1.0.0. The base protocol stays 7 and UI Document.version stays 1; package versions change separately. It extends the existing ui.richtext ^1 Kind::CodeBlock { text, language } without replacing that capability or granting editing, file reads, native processes or language-server startup.

## Declaration and authority

Document.code_highlighting defaults false. Disabled CodeBlock remains native read-only monospace text and requests no highlighting. When enabled, the manifest negotiates ui.code_highlighting and ui.richtext, declares and receives editor.read, and publishes from its own declared workspace editor panel. Application instances and other panels cannot borrow workspace authority.

Document.source: DocumentVersion must echo the exact document ID, workspace-relative path and revision of that panel's latest authorized Preview. Source text arrives as versioned memory notifications; the plugin declares derived code and language, not a file read through this capability. Publication and asynchronous adoption each revalidate ownership and revision.

```json
{
  "version": 1,
  "revision": 12,
  "source": { "id": "open-document-1", "path": "notes.sample", "revision": 4 },
  "code_highlighting": true,
  "root": {
    "id": "example-code",
    "kind": { "type": "code_block", "text": "let value = 1;\n", "language": "rust" }
  }
}
```

This requests an already selected rust highlighting provider, not installation or tool startup. Ordinary node, code text and whitespace quotas still apply. Missing highlighting/richtext capability returns CapabilityUnavailable; missing read/panel authority PermissionDenied; an enabled document without source InvalidRequest; a stale/wrong source StaleRevision. Rejected publication never replaces the active view.

## Providers and rendering

Only installed, enabled and effectively selected dynamic Tree-sitter WASM grammars, queries and parser factories are reused. Recognition and highlighting providers remain independent and follow [language package](languages.md) selection/replacement; no host grammar is built in. language first matches a public language ID and may match a declared display name or extension. Ambiguity follows recognition selection or falls back. The host contains no language-specific alias table; domain aliases belong to the plugin.

Missing/unknown language, absent/disabled provider, loading/parsing failure or exhausted budget leaves that block's monospace text and code styling intact. It does not reject an otherwise valid view or lose text, spaces or newlines. It starts no extra LSP and does not implicitly read HTML, URLs or code contents.

Declared static injections follow the same language contract, using an independently selected WASM provider per allowed target. Missing/disabled targets leave just that range plain; target selection/replacement/disable revokes old batches. Main grammar and injections share capture, text, naming, cancellation and deadline budgets; cyclic or overly deep injection falls back.

Background work returns only bounded read-only capture ranges and semantic names. Native painting resolves current theme colors/styles, so cached colors cannot survive theme changes. Unknown capture styles retain ordinary code appearance. Guests receive no GPUI object, parser handle or second mutable document. Preview owns no source selection, IME or Undo/Redo.

## Lifecycle and limits

Results bind preview instance, panel, source ID/path/revision, UI scene revision, node identity, exact code/language and selected provider version/generation. Source switch/edit/close, preview revocation, plugin disable/uninstall, workspace trust revocation and provider change/retirement revoke pending work and caches. Cancel releases batch/quota resources; late results are discarded.

Reopening a path, re-enabling the same provider identity or restoring identical text cannot revive an old task: document entities and instance/provider generations must still match. Theme changes may redraw still-valid semantic captures but cannot revive retired parsing.

| Resource | Limit |
| --- | --- |
| Global active highlighting batches | 4 |
| In-flight batches per preview | 1 |
| Parsed blocks per scene | 64 |
| Retained capture tokens per scene | 16,384 |
| Code input per block | 64 KiB UTF-8 |
| Capture tokens per block | 4,096 |
| Capture name | 128 UTF-8 bytes, checked before copying |
| Cumulative raw/final capture names per block | 64 KiB each, checked before copying |
| Injection depth/parser layers | 4/32 per block |
| Injection content events per block | 4,096 |
| Physical injection included ranges per block | 4,096 |
| Cumulative injection included ranges per block | 64 KiB |
| Batch deadline, including queueing | 30 seconds |

Exhaustion falls back per block without changing source or native-view authority. These limits supplement the full UI's 2 MiB encoding, 1 MiB text and 2048-node limits. No valid provider means no implicit installation or unbounded retry queue.
