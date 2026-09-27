# 示例 WebAssembly 插件

本目录包含计数器与笔记插件的完整实现：`src/` 为代码，`Cargo.toml` 为 WASM crate，`manifest.json` 声明面板与命令。共享宿主协议位于 `crates/plugin-protocol`。

在仓库根目录执行 `./scripts/build-plugins.ps1`，生成标准 ZIP 包 `dist/plugins/example.zip`，然后通过编辑器的插件管理界面安装。Cargo 包名仍为 `example-guest`。
