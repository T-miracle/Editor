# 插件 SDK

跨插件协作、版本化契约、提供者选择及来源权限见 [SERVICES.md](SERVICES.md)。

原生服务、交互式进程、权限与回收契约见 [PROCESSES.md](PROCESSES.md)。

纯声明式语言包及独立识别、高亮提供者的格式见 [LANGUAGES.md](LANGUAGES.md)。无需附带空生命周期组件；安装、启停、更新和卸载会同步到已打开文档。

## 当前能力协议

协议传输标记 `protocol = 7` 选择 `api` 模块的类型化消息；它不是所有功能共用的接口版本。清单 `api.base` 使用基础协议 SemVer 范围，`api.required` / `api.optional` 分别声明能力 ID 与版本范围。当前基础 API 为 1.0.0，支持 `package.assets` 1.0.0 和 `ui.native` 1.0.0；插件包版本仍单独管理。必需接口缺失或版本不匹配在包检查及实例恢复时拒绝，可选接口不可用则不出现在 Prepare 的协商结果中。

插件使用 `api::guest::dispatch` 接收类型化生命周期，并用 `api::guest::read_asset` 读取资源。SDK 自动生成非零请求 ID、编码消息和校验响应 ID。缺少方法、错误参数、未知操作、未协商能力、权限不足、非法路径与配额超限以 `Failure` 返回；真正无法解码或调用的组件传输错误仍走 WIT 错误。同步资源读取直接返回最终结果，不伪造“处理中”阶段。

`package.assets` 的可用性不等于授权：清单还需声明并在安装时批准 `assets.read`。该操作只读取当前包版本内的资源，准备阶段也允许读取这些不可变资源；不能读工作区或其他插件目录。

新 UI 输出使用 `api::View { panel, document }`，普通表单不要求画布或字符网格字段。宿主检查 `ui.native` 是否已协商、面板是否已声明；`ui.canvas ^1` 允许在同一树任意位置放置画布，`ui.grid ^1` 单独提供可选字符测量。原生通知保留面板和节点作用域。编辑区预览使用 `editor.documents` 与 `editor.read`，以带版本的内存文本通知驱动；旧画布/PTY 消息不会自动成为新基础协议的一部分。组合协议详见 [UI.md](UI.md)。

当前包安装和实例恢复要求 `protocol = 7` 及可协商的基础 API。协议 1–6 的已安装记录保留设置、权限、启用范围和私有数据，并显示需要更新；旧组件不会激活。使用当前 SDK 重建并安装同一插件的新包后恢复使用。协议编解码器和个别旧测试暂留供开发迁移，不能作为运行兼容承诺；最终删除由收缩工单 #21 负责。

独立验证插件为 `capability-example`，不属于正式发行包。先构建开发版宿主，再运行 `scripts/verify-plugin-sdk.ps1`：脚本将示例源文件、清单、README 和资源复制到系统临时目录，从该目录通过实际宿主的公开 `--plugin-cargo` 命令构建，不使用宿主仓库的 Cargo 工作区或业务源码路径。脚本还通过 `--export-plugin-sdk` 验证完整导出与损坏文件恢复。验证命令：

```powershell
cargo build -p editor-app
./scripts/verify-plugin-sdk.ps1 -HostExe ./target/debug/editor-app.exe
cargo test -p plugin-runtime --test sdk_distribution -- --ignored
cargo test -p plugin-runtime --test capability_packages
cargo test -p plugin-runtime --test capability_packages -- --ignored
cargo test -p editor-app --bin editor-app capability_package_consent -- --ignored
```

真实组件测试显式忽略默认运行，需要先生成测试包；不是跳过验收。脚本将同一 ZIP 输出至 `target/plugin-sdk-test/capability-example.zip` 与既有测试使用的 `target/plugin-api-test/capability-example.zip`，包内仅有清单、README、组件和声明资源。`sdk_distribution` 通过公开 `Package` / `Manager` 接口读取 README、安装真实组件并调用类型化错误诊断命令。日常快速构建仍可使用 `scripts/build-capability-example.ps1`，但仓库外分发验收以 `verify-plugin-sdk.ps1` 为准。

## configuration 1.0

协议 7 清单的 `settings` 以插件内部键声明 `title`、`value_type`、`default`、`scope` 和 `apply`。类型为 boolean、string（max_length）、integer（min/max）、enum（choices）；作用域为 user 或 project，后者允许明确确认的项目覆盖。此版本的生效方式为 `restart_instance`。最多 64 个字段、字符串最多 4096 字节、枚举最多 32 个不重复选项；无效默认值在包检查时拒绝。可执行包需声明必需能力 `configuration: ^1`。

可选 `settings_hook: true` 接收 `Notification::Configuration { phase: Validate, values }`，通过 `Output.configuration: Proposal` 返回 discovered 和 errors。钩子在候选实例的准备阶段运行，继承有限指令与内存预算，不能获取活动实例资源或授权；读取已获权限的包资源仍可用。发现值只填充未显式设置的项，任何错误使应用失败。随后 Apply 阶段收到带 Source 的最终值，供 Activate 使用；此阶段失败也不替换旧实例。配置被限制在插件命名空间，访客不能经配置获得权限或修改宿主用户设置。

用户设置独立于访客私有文件，已确认项目值保存在宿主管理的工作区记录，不自动信任仓库文件。热应用、初始化与再次打开工作区使用相同解析路径。应用失败保留先前配置和运行实例；成功只替换相关实例并撤销其旧资源。

## 独立构建与 SDK 分发

本目录是主程序持有的版本化插件接口。`wit/plugin.wit` 定义 WebAssembly Component Model 的导入与导出；Rust crate `plugin-protocol` 定义通过该接口传递的 JSON 消息、场景和权限名称。主程序把这些接口文件编入可执行文件，并自动管理插件编译所需的接口缓存。

`host.request` 是主程序在运行时提供的导入。当前插件通过 `api::guest` 使用类型化能力请求，主程序逐次检查协商能力、插件权限、实例作用域和句柄。WIT 和 Rust 类型只在编译插件时使用，安装后的 `.wasm` 不读取 SDK 文件。

插件在 `Cargo.toml` 声明 `plugin-protocol = { version = "=0.1.0", features = ["guest"] }`，通过 `plugin_protocol::bindings::{Guest, editor, export}` 使用宿主调用和组件导出，无需自行生成 WIT 绑定。独立编译时直接调用已打包的编辑器：

```powershell
editor-app.exe --plugin-cargo capability-example/Cargo.toml build --target wasm32-wasip2 --release
# 同一入口也支持原生单元测试和编译检查。
editor-app.exe --plugin-cargo capability-example/Cargo.toml test --lib
editor-app.exe --plugin-cargo capability-example/Cargo.toml check --target wasm32-wasip2
```

编辑器将内嵌接口按内容摘要缓存到系统用户缓存目录的 `MeEditor/plugin-sdk/<摘要>/`（Windows 为 `%LOCALAPPDATA%/MeEditor/plugin-sdk/<摘要>/`），通过本次 Cargo 命令的依赖覆盖选择该缓存。不同接口版本不会互相覆盖；缓存缺失或损坏会自动恢复。插件项目不需要 `sdk/`，也不引用主程序源码；发行目录只需主程序与插件包。接口变更时仍需核对 WIT 包版本和插件清单的 `protocol` 版本。

开发机器需安装 Rust/Cargo 与 `wasm32-wasip2` 目标；使用已编译插件的用户无需这些工具。`--plugin-cargo` 执行开发者提供的 Cargo 项目，属于本机开发工具，不是运行时沙箱。为其他语言工具链或接口检查保留显式 `--export-plugin-sdk <目录>`，常规构建与发行不调用它。

在本编辑器内开发插件时，Rust 语言服务自动使用与 `--plugin-cargo` 相同的宿主 SDK 缓存。编辑器通过 Rust Analyzer 的 `cargo.configPath` 注入依赖覆盖，并通过 `linkedProjects` 加入工作区内同时包含 `manifest.json` 与 `Cargo.toml` 的独立插件项目；发现过程遵循忽略规则，跳过构建产物和 vendor 目录。输入 `plugin_protocol::api::` 可以获取类型补全、悬浮说明和定义跳转。插件目录无需增加 SDK、Cargo 配置或指向宿主源码的路径；定义跳转会打开宿主缓存中的协议源码。需要已启用 Rust 语言插件并安装 Rust Analyzer。

## 开发过渡中的历史类型

SDK 暂留的 `Input`、`Event`、`Request` 及旧画布传输类型仅服务于迁移开发与历史数据检查。新插件使用 `api::Input`、`api::Notification`、`api::Operation` 和协商能力；原生 UI 树继续使用共享的 `ui` 模块。保留 Rust 类型不代表允许安装或执行协议 1–6 的组件。
