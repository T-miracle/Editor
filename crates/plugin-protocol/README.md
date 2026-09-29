# 插件 SDK

原生界面协议与示例见 [UI.md](UI.md)。`plugin_protocol::ui` 提供布局、控件、主题角色、弹窗与事件，使用它的插件声明 `protocol = 2`；宿主仍兼容 protocol 1 画布插件。

本目录是主程序持有的版本化插件接口。`wit/plugin.wit` 定义 WebAssembly Component Model 的导入与导出；Rust crate `plugin-protocol` 定义通过该接口传递的 JSON 消息、场景和权限名称。主程序把这些接口文件编入可执行文件，并自动管理插件编译所需的接口缓存。

`host.request` 是主程序在运行时提供的导入。插件通过它请求 PTY、工作区、私有存储、剪贴板和编辑器操作；主程序逐次检查插件权限。WIT 和 Rust 类型只在编译插件时使用，安装后的 `.wasm` 不读取 SDK 文件。

插件在 `Cargo.toml` 声明 `plugin-protocol = { version = "=0.1.0", features = ["guest"] }`，通过 `plugin_protocol::bindings::{Guest, editor, export}` 使用宿主调用和组件导出，无需自行生成 WIT 绑定。独立编译时直接调用已打包的编辑器：

```powershell
editor-app.exe --plugin-cargo terminal/Cargo.toml build --target wasm32-wasip2 --release
# 同一入口也支持原生单元测试和编译检查。
editor-app.exe --plugin-cargo terminal/Cargo.toml test --lib
editor-app.exe --plugin-cargo terminal/Cargo.toml check --target wasm32-wasip2
```

编辑器将内嵌接口按内容摘要缓存到系统用户缓存目录的 `MeEditor/plugin-sdk/<摘要>/`（Windows 为 `%LOCALAPPDATA%/MeEditor/plugin-sdk/<摘要>/`），通过本次 Cargo 命令的依赖覆盖选择该缓存。不同接口版本不会互相覆盖；缓存缺失或损坏会自动恢复。插件项目不需要 `sdk/`，也不引用主程序源码；发行目录只需主程序与插件包。接口变更时仍需核对 WIT 包版本和插件清单的 `protocol` 版本。

开发机器需安装 Rust/Cargo 与 `wasm32-wasip2` 目标；使用已编译插件的用户无需这些工具。`--plugin-cargo` 执行开发者提供的 Cargo 项目，属于本机开发工具，不是运行时沙箱。为其他语言工具链或接口检查保留显式 `--export-plugin-sdk <目录>`，常规构建与发行不调用它。
