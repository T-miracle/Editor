# 插件 SDK

纯声明式语言包及独立识别、高亮提供者的格式见 [LANGUAGES.md](LANGUAGES.md)。无需附带空生命周期组件；安装、启停、更新和卸载会同步到已打开文档。

## 新能力协议开发切片

协议传输标记 `protocol = 7` 选择 `api` 模块的类型化消息；它不是所有功能共用的接口版本。清单 `api.base` 使用基础协议 SemVer 范围，`api.required` / `api.optional` 分别声明能力 ID 与版本范围。当前基础 API 为 1.0.0，支持 `package.assets` 1.0.0 和 `ui.native` 1.0.0；插件包版本仍单独管理。必需接口缺失或版本不匹配在包检查及实例恢复时拒绝，可选接口不可用则不出现在 Prepare 的协商结果中。

插件使用 `api::guest::dispatch` 接收类型化生命周期，并用 `api::guest::read_asset` 读取资源。SDK 自动生成非零请求 ID、编码消息和校验响应 ID。缺少方法、错误参数、未知操作、未协商能力、权限不足、非法路径与配额超限以 `Failure` 返回；真正无法解码或调用的组件传输错误仍走 WIT 错误。同步资源读取直接返回最终结果，不伪造“处理中”阶段。

`package.assets` 的可用性不等于授权：清单还需声明并在安装时批准 `assets.read`。该操作只读取当前包版本内的资源，准备阶段也允许读取这些不可变资源；不能读工作区或其他插件目录。旧协议的隐式包资源读取规则只保留在旧传输分支。

新 UI 输出使用 `api::View { panel, document }`，不要求插件填写画布字体、光标或字符网格字段。宿主检查 `ui.native` 是否已协商、面板是否已声明，再交给现有原生渲染器。原生通知保留面板作用域，只包含原生 UI、主题、命令、焦点和像素尺寸；旧画布/PTY 消息不会自动成为新基础协议的一部分。

独立验证插件为 `capability-example`，不属于正式发行包。先构建开发版宿主，再运行 `scripts/build-capability-example.ps1`，通过宿主公开 `--plugin-cargo` 接口生成开发测试包。验证命令：

```powershell
cargo test -p plugin-runtime --test capability_packages
cargo test -p plugin-runtime --test capability_packages -- --ignored
cargo test -p editor-app --bin editor-app capability_package_consent -- --ignored
```

真实组件测试显式忽略默认运行，需要先生成测试包；不是跳过验收。开发期间协议 1–6 与新形式并存，以保持迁移批次之间可运行。旧包统一迁移和旧接口删除属于后续工单，当前不承诺永久兼容，也不发布中间双协议正式版本。

## configuration 1.0

协议 7 清单的 `settings` 以插件内部键声明 `title`、`value_type`、`default`、`scope` 和 `apply`。类型为 boolean、string（max_length）、integer（min/max）、enum（choices）；作用域为 user 或 project，后者允许明确确认的项目覆盖。此版本的生效方式为 `restart_instance`。最多 64 个字段、字符串最多 4096 字节、枚举最多 32 个不重复选项；无效默认值在包检查时拒绝。可执行包需声明必需能力 `configuration: ^1`。

可选 `settings_hook: true` 接收 `Notification::Configuration { phase: Validate, values }`，通过 `Output.configuration: Proposal` 返回 discovered 和 errors。钩子在候选实例的准备阶段运行，继承有限指令与内存预算，不能获取活动实例资源或授权；读取已获权限的包资源仍可用。发现值只填充未显式设置的项，任何错误使应用失败。随后 Apply 阶段收到带 Source 的最终值，供 Activate 使用；此阶段失败也不替换旧实例。配置被限制在插件命名空间，访客不能经配置获得权限或修改宿主用户设置。

用户设置独立于访客私有文件，已确认项目值保存在宿主管理的工作区记录，不自动信任仓库文件。热应用、初始化与再次打开工作区使用相同解析路径。应用失败保留先前配置和运行实例；成功只替换相关实例并撤销其旧资源。

## 迁移期间的旧接口（协议 1–6）

原生界面协议与示例见 [UI.md](UI.md)。`plugin_protocol::ui` 提供布局、控件、主题角色、弹窗与事件，使用它的插件声明 `protocol = 2`；宿主仍兼容 protocol 1 画布插件。

画布组合控件使用 `CanvasControls/SideTabs`；左右停靠使用 protocol 5 的 `SideTabs.position`，省略位置的旧包保持右侧停靠。编译提示由新版宿主内嵌的 SDK 自动提供，无需在插件项目复制接口文件。

本目录是主程序持有的版本化插件接口。`wit/plugin.wit` 定义 WebAssembly Component Model 的导入与导出；Rust crate `plugin-protocol` 定义通过该接口传递的 JSON 消息、场景和权限名称。主程序把这些接口文件编入可执行文件，并自动管理插件编译所需的接口缓存。

`host.request` 是主程序在运行时提供的导入。插件通过它请求 PTY、工作区、私有存储、剪贴板和编辑器操作；主程序逐次检查插件权限。WIT 和 Rust 类型只在编译插件时使用，安装后的 `.wasm` 不读取 SDK 文件。

获授 `editor.commands` 的插件可调用 `Request::Editor { command: "hide_panel:<panel-id>".into() }` 隐藏自己的已声明面板。原生宿主按请求插件 ID 查找面板，不能隐藏其他插件或编辑器内置面板；隐藏状态写入编辑器会话，面板大小与插件实例仍保留。该请求异步执行，不产生 `.result` 回调。插件自行决定何时隐藏及如何管理会话。

用户通过面板按钮打开窗口、或宿主调用命令并显示面板时，会发送作用域为该 `Surface` 的 `Event::Command { id: "panel.opened", ... }`，无需插件在清单声明这个生命周期事件。插件据此初始化空界面，已有状态保持不变；同一次打开可能伴随普通焦点事件，插件不应因此重复初始化。

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
