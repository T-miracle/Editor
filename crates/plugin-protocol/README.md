# 插件 SDK

原生界面协议与示例见 [UI.md](UI.md)。`plugin_protocol::ui` 提供布局、控件、主题角色、弹窗与事件，使用它的插件声明 `protocol = 2`；宿主仍兼容 protocol 1 画布插件。

本目录是主程序持有的版本化插件接口。`wit/plugin.wit` 定义 WebAssembly Component Model 的导入与导出；Rust crate `plugin-protocol` 定义通过该接口传递的 JSON 消息、场景和权限名称。主程序把这些接口文件编入可执行文件，并自动管理插件编译所需的接口缓存。

`host.request` 是主程序在运行时提供的导入。插件通过它请求 PTY、工作区、私有存储、剪贴板和编辑器操作；主程序逐次检查插件权限。WIT 和 Rust 类型只在编译插件时使用，安装后的 `.wasm` 不读取 SDK 文件。

宿主主动调用插件使用 `Event::Command`。可选 `arguments` 是插件自行定义的 JSON 参数；旧宿主省略该字段仍可反序列化，旧插件也可忽略新字段。原有 `cwd`、`text` 字段继续用于编辑器操作回调。该扩展保持现有 JSON/WIT 接口及清单协议版本，宿主不引入终端等功能的专属类型。`plugin_runtime::Manager::invoke_command(plugin_id, command_id, arguments)` 检查已运行插件、声明命令及 64 KiB 参数上限，随后沿原有串行事件和权限检查执行；编辑器 UI 通过 `EditorApp::invoke_plugin_command` 异步排队并显示插件面板。

插件在 `Cargo.toml` 声明 `plugin-protocol = { version = "=0.1.0", features = ["guest"] }`，通过 `plugin_protocol::bindings::{Guest, editor, export}` 使用宿主调用和组件导出，无需自行生成 WIT 绑定。独立编译时直接调用已打包的编辑器：

```powershell
editor-app.exe --plugin-cargo terminal/Cargo.toml build --target wasm32-wasip2 --release
# 同一入口也支持原生单元测试和编译检查。
editor-app.exe --plugin-cargo terminal/Cargo.toml test --lib
editor-app.exe --plugin-cargo terminal/Cargo.toml check --target wasm32-wasip2
```

编辑器将内嵌接口按内容摘要缓存到系统用户缓存目录的 `MeEditor/plugin-sdk/<摘要>/`（Windows 为 `%LOCALAPPDATA%/MeEditor/plugin-sdk/<摘要>/`），通过本次 Cargo 命令的依赖覆盖选择该缓存。不同接口版本不会互相覆盖；缓存缺失或损坏会自动恢复。插件项目不需要 `sdk/`，也不引用主程序源码；发行目录只需主程序与插件包。接口变更时仍需核对 WIT 包版本和插件清单的 `protocol` 版本。

开发机器需安装 Rust/Cargo 与 `wasm32-wasip2` 目标；使用已编译插件的用户无需这些工具。`--plugin-cargo` 执行开发者提供的 Cargo 项目，属于本机开发工具，不是运行时沙箱。为其他语言工具链或接口检查保留显式 `--export-plugin-sdk <目录>`，常规构建与发行不调用它。

在本编辑器内开发插件时，Rust 语言服务自动使用与 `--plugin-cargo` 相同的宿主 SDK 缓存。编辑器通过 Rust Analyzer 的 `cargo.configPath` 注入依赖覆盖，并通过 `linkedProjects` 加入工作区内同时包含 `manifest.json` 与 `Cargo.toml` 的独立插件项目；发现过程遵循忽略规则，跳过构建产物和 vendor 目录。输入 `plugin_protocol::ui::` 可以获取类型补全、悬浮说明和定义跳转。插件目录无需增加 SDK、Cargo 配置或指向宿主源码的路径；定义跳转会打开宿主缓存中的协议源码。需要已启用 Rust 语言插件并安装 Rust Analyzer。
