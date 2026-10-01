# TOML 插件

为 `.toml` 文件以及明确列出的 `Cargo.lock`、`uv.lock` 提供 Tree-sitter WASM 语法高亮和文件图标。其他 `.lock` 文件不会自动按 TOML 解析。此插件只提供声明式资源，不启动独立的 WASM 组件。
