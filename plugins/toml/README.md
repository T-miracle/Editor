# TOML 插件

版本 0.2.0 使用协议 7，为 `.toml`、`Cargo.lock` 和 `uv.lock`（其他 `.lock` 文件不参与识别） 提供独立的语言识别、Tree-sitter 高亮和文件图标。语法 provider 与语言服务选择独立；安装、停用、替换及卸载后已打开文件会重新选择可用 provider。

本包只有声明与资源，不包含生命周期 WASM 组件或语言服务。`grammar/toml.wasm` 是语法资源，不是动态插件组件。

通过 `scripts/build-plugins.ps1 -HostExe <editor-app.exe>` 打包；ZIP 保留本 README、清单、查询、图标和原始 grammar。
