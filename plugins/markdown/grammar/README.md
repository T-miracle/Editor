# Grammar 来源

使用 [tree-sitter-markdown v0.5.3](https://github.com/tree-sitter-grammars/tree-sitter-markdown/releases/tag/v0.5.3) 发布的原始 WASM 资源，Tree-sitter ABI 15，MIT 许可。高亮查询基于同版本公开节点定义。

| 资源 | SHA-256 |
| --- | --- |
| markdown.wasm | dd9fc12ac2804d7c7da787e4774125b32e4fb3c244e5e7031a77cb7dd8036020 |
| markdown_inline.wasm | d47e5c43683c39b645ced8720ab90c3b3f467e715d3426b29ef3f69984d88c15 |

块级与行内 grammar 分开发布，不是插件生命周期组件，不向宿主引入原生 grammar。
