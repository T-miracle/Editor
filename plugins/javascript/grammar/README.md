# JavaScript Tree-sitter WASM grammar

`javascript.wasm` is copied from `out/tree-sitter-javascript.wasm` in
[`@repomix/tree-sitter-wasms@0.1.17`](https://www.npmjs.com/package/@repomix/tree-sitter-wasms/v/0.1.17).
The distribution declares `tree-sitter-javascript ^0.25.0`; the bundled grammar
exports Tree-sitter ABI 15.

WASM SHA-256:

`06ad932f038e999733c8aae0b7ff0c8bd22953c10a681d39c69c77e6e707c90e`

The highlighter combines `highlights.scm`, `highlights-jsx.scm`, and
`highlights-params.scm` from
[`tree-sitter-javascript@0.25.0`](https://github.com/tree-sitter/tree-sitter-javascript/tree/v0.25.0/queries).
Both pinned npm tarballs were checked against their registry SHA-512 integrity
values before extraction. No upstream install scripts were executed.

The grammar and queries use the MIT license (`LICENSE.tree-sitter-javascript`).
The prebuilt distribution uses the Unlicense (`LICENSE.tree-sitter-wasms`).
