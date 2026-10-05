---
title: Language packages
description: Declarative recognition and highlighting contributions, provider selection and validation rules.
section: sdk
order: 6
alternate: /zh-cn/sdk/languages/
---

# Declarative language packages

A protocol 7 package may omit `component`. Its `manifest.json` sets `contributions` to a
package-relative TOML file, declares `api.base = "^1"`, and uses empty permissions, panels
and commands. Recognition and highlighting require no executable lifecycle guest.

```json
{"id":"novel-language","name":"Novel language","version":"1.0.0","protocol":7,"api":{"base":"^1"},"contributions":"plugin.toml","storage_limit":1024}
```

The contribution metadata must match the package ID and version:

```toml
[plugin]
id = "novel-language"
name = "Novel language"
version = "1.0.0"
host_version = ">=0.1.0"

[[language_definitions]]
id = "novel"
name = "Novel"
extensions = ["novel"]
filenames = ["Novelconfig"]

[[highlighters]]
id = "syntax"
language = "novel"
grammar_name = "toml"
grammar = "grammar/toml.wasm"
highlights = "queries/highlights.scm"
tree_sitter_abi = 15
```

This example recognises a new language identity while reusing compatible TOML syntax.
`grammar_name` is the Tree-sitter module export name and is independent of the document
language ID. Ship the real WASM grammar and query at those paths; the host validates their
ABI and query before publication. No native grammar fallback is enabled when validation
fails.

Each array may hold up to 64 entries. Language and highlighter IDs use lowercase ASCII
letters, digits, dots, underscores or hyphens and are at most 100 bytes. The identifier
`text` is reserved for inert plain text. Each language has a non-empty name, at least one
selector and at most 128 selectors. Extensions omit the dot and filenames are basenames.
Matching is case-insensitive, exact filenames take priority, and equivalent duplicate
selectors within one definition are rejected. The schema currently accepts Tree-sitter ABI 14
or 15, and runtime validation also checks the module's actual ABI. All asset paths stay within
the immutable package directory.

One package may contribute several languages, recognition alone, highlighting alone, or both.
Providers from different packages can be combined. Recognition choices are keyed by file
selector and highlighting choices by language ID. A provider identity is its package ID plus
its contribution ID, independent of the package version.

The native **Settings → Language providers** page selects user preferences, or confirmed
local project preferences, separately for recognition and highlighting. A project choice
overrides the user choice. A sole provider is adopted automatically and remembered; adding
competitors keeps the existing valid choice. Removing the chosen provider adopts a sole
remaining candidate, asks the user when several remain, or reports a plain-text fallback when
none remain. Reset removes the selected preference layer and relinquishes its remembered
choice before resolving again.

Choices live in the host plugin management directory, outside guest private data and project
files, and they grant no permissions or workspace trust. Disabled, failed or untrusted
contributions are withdrawn; lifecycle changes refresh open editors without reopening files.
Background loading only prepares grammar data. Registration follows checks of the active task
generation and the selected package version, so retired work cannot reinstall a parser.

File icons and themes keep their existing declarations. Combined legacy `languages`
declarations are rejected: use the independent arrays above and declare language servers
through the versioned LSP capability. Recognition and highlighting never start a native
process.
