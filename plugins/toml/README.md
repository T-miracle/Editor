# TOML 插件

版本 0.2.2 使用协议 7，为 `.toml`、`Cargo.lock` 和 `uv.lock`（其他 `.lock` 文件不参与识别） 提供独立的语言识别、Tree-sitter 高亮和文件图标。语法 provider 与语言服务选择独立；安装、停用、替换及卸载后已打开文件会重新选择可用 provider。

本包只有声明与资源，不包含生命周期 WASM 组件或语言服务。`grammar/toml.wasm` 是语法资源，不是动态插件组件。

通过宿主 `editor-app.exe --plugin-package plugins/toml` 完整构建并封装 ZIP，默认输出为插件项目根部的 `toml-0.2.2.zip`。项目描述在 `nanobug-plugin.json`，也可使用宿主“插件打包”配置选择输出位置；不调用归档脚本。
