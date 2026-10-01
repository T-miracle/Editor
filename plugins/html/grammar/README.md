# Bundled HTML grammar

`html.wasm` is the unmodified `tree-sitter-html.wasm` distributed by the upstream
[`tree-sitter-html` 0.23.2 npm package](https://www.npmjs.com/package/tree-sitter-html/v/0.23.2).
Source: [tree-sitter/tree-sitter-html](https://github.com/tree-sitter/tree-sitter-html/tree/v0.23.2).

The package tarball was downloaded from
`https://registry.npmjs.org/tree-sitter-html/-/tree-sitter-html-0.23.2.tgz`
and checked against the registry's SHA-512 integrity before extraction:

```text
sha512-TN+l+7cCeLx9db/1RhRSqMAZO/266Oh2BHb8J8hMSSFLuzYvFTYP/UnD3S0mny5awzw05KzFNgu2vnwzN9wVJg==
```

The grammar exports Tree-sitter ABI 14. SHA-256 of `html.wasm`:

```text
c48fcd82c7ea8bf943180088ba7f28c48b2bb5287874179168bf9d31e394cf85
```

The grammar and adapted highlight query are MIT licensed; the upstream notice is
included as `LICENSE.tree-sitter-html`. The query additionally captures quoted
attribute values, character entities, and assignment operators.
