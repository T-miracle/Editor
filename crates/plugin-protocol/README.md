# 插件 SDK

原生界面协议与示例见 [UI.md](UI.md)。`plugin_protocol::ui` 提供布局、控件、主题角色、弹窗与事件，使用它的插件声明 `protocol = 2`；宿主仍兼容 protocol 1 画布插件。

本目录是主程序持有的版本化插件接口。`wit/plugin.wit` 定义 WebAssembly Component Model 的导入与导出；Rust crate `plugin-protocol` 定义通过该接口传递的 JSON 消息、场景和权限名称。主程序把这些接口文件编入可执行文件，发行后由可执行文件导出给插件开发者。

`host.request` 是主程序在运行时提供的导入。插件通过它请求 PTY、工作区、私有存储、剪贴板和编辑器操作；主程序逐次检查插件权限。WIT 和 Rust 类型只在编译插件时使用，安装后的 `.wasm` 不读取 SDK 文件。

已打包的主程序可执行 `editor-app.exe --export-plugin-sdk <目标/sdk目录>` 导出接口。独立编译时，把导出的 `sdk/` 放在插件目录的同级，例如 `terminal/` 与 `sdk/`，再执行：

```powershell
cargo build --manifest-path terminal/Cargo.toml --target wasm32-wasip2 --release
```

发行脚本先构建主程序，再通过上述命令导出 `dist/editor/sdk/`。插件只使用这份导出物；接口变更时要核对 WIT 包版本和插件清单的 `protocol` 版本。
