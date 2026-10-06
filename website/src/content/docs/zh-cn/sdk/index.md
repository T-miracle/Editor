---
title: 插件 SDK
description: 协商能力、类型化消息、原生界面、打包与 SDK 的交付方式。
section: sdk
order: 0
alternate: /en/sdk/
---

# 插件 SDK

跨插件协作、版本化契约、提供者选择及来源权限见 [SERVICES.md](/zh-cn/sdk/services/)。

原生服务、交互式进程、权限与回收契约见 [PROCESSES.md](/zh-cn/sdk/processes/)。

纯声明式语言包及独立识别、高亮提供者的格式见 [LANGUAGES.md](/zh-cn/sdk/languages/)。无需附带空生命周期组件；安装、启停、更新和卸载会同步到已打开文档。

## 当前能力协议

协议传输标记 `protocol = 7` 选择 `api` 模块的类型化消息；它不是所有功能共用的接口版本。清单 `api.base` 使用基础协议 SemVer 范围，`api.required` / `api.optional` 分别声明能力 ID 与版本范围。当前基础 API 为 1.0.0，支持 `package.assets` 1.0.0 和 `ui.native` 1.0.0；插件包版本仍单独管理。必需接口缺失或版本不匹配在包检查及实例恢复时拒绝，可选接口不可用则不出现在 Prepare 的协商结果中。

插件使用 `api::guest::dispatch` 接收类型化生命周期，并用 `api::guest::read_asset` 读取资源。SDK 自动生成非零请求 ID、编码消息和校验响应 ID。缺少方法、错误参数、未知操作、未协商能力、权限不足、非法路径与配额超限以 `Failure` 返回；真正无法解码或调用的组件传输错误仍走 WIT 错误。同步资源读取直接返回最终结果，不伪造“处理中”阶段。

`package.assets` 的可用性不等于授权：清单还需声明并在安装时批准 `assets.read`。该操作只读取当前包版本内的资源，准备阶段也允许读取这些不可变资源；不能读工作区或其他插件目录。

新 UI 输出使用 `api::View { panel, document }`，普通表单不要求画布或字符网格字段。宿主检查 `ui.native` 是否已协商、面板是否已声明；`ui.canvas ^1` 允许在同一树任意位置放置画布，`ui.grid ^1` 单独提供可选字符测量。原生通知保留面板和节点作用域。编辑区预览使用 `editor.documents` 与 `editor.read`，以带版本的内存文本通知驱动；旧画布/PTY 消息不会自动成为新基础协议的一部分。组合协议详见 [UI.md](/zh-cn/sdk/ui/)。

当前包安装和实例恢复要求 `protocol = 7` 及可协商的基础 API。协议 1–6 的已安装记录保留设置、权限、启用范围和私有数据，并显示需要更新；旧组件不会激活。使用当前 SDK 重建并安装同一插件的新包后恢复使用。旧运行协议与转换器已经删除；旧安装记录只参与管理界面展示和有限数据导入，不恢复执行。

## 运行、调试与目标服务

[运行会话](/zh-cn/sdk/sessions/)说明公开输入、展示、输出订阅与正常／强制停止；[调试会话](/zh-cn/sdk/debug/)说明暂停代次、真实检查及断点验证；[运行目标提供者](/zh-cn/sdk/targets/)贡献可移植绑定并受控准备产物。

[配置模板](/zh-cn/sdk/configurations/)为本机配置草稿提供插件默认值、原生表单和校验。

## workspace.files 1.1 与 host.sdk 1.0

`api::guest::find_files(&workspace, FileQuery { include, exclude, max_results })` 使用 `open_workspace` 返回的工作区根句柄，并在每次调用时重新检查 `workspace.files >=1.1` 与 `workspace.read`。私有数据、其他实例、已释放和已退役的句柄不能用于发现。应用作用域插件不拥有工作区根；调用不会跟随当前选中的其他工作区。

查询使用根相对、`/` 分隔的 glob；`**/` 可以匹配零级目录，`*` 不跨目录。绝对路径、驱动器前缀、反斜杠、空段、`.` 和 `..` 段无效。`include` 至少一项，include/exclude 合计最多 32 项，每项最多 1024 字节；`max_results` 为 1–4096。exclude 在目录入口剪枝，例如 `**/generated/**` 不会进入 generated 目录。宿主不内置语言项目或构建目录名称，由插件声明选择规则。

发现遵循根内 `.gitignore` 和 `.ignore` 的嵌套与否定规则，`.ignore` 优先；不读取工作区外的父级或全局 ignore，不跟随符号链接、Windows junction 或其他 reparse point。`FileMatches.paths` 是去重、排序的 UTF-8 工作区相对路径；`skipped` 是无法读取或解析的相对路径，调用者据此决定是否接受不完整发现。受限预算包括 50,000 个目录条目（含忽略项）、64 级目录深度、512 KiB 编码结果及最多 64 项 skipped；ignore 文件单个最多 64 KiB、合计 512 KiB/2048 行。超限返回 `LimitExceeded`，不会成功返回截断列表；遍历在文件系统调用之间检查当前插件调用期限，超时返回 `TimedOut`。

`api::guest::describe_sdk()` 需要协商 `host.sdk`，返回 `SdkDescriptor { digest, root, cargo_config }`，无需工作区读取权限。它描述宿主持有的同一份接口缓存：digest 是内容标识，root 与 cargo_config 是供原生语言工具使用的绝对路径。它不创建文件句柄、WASI preopen 或额外文件读取权限；将这些路径交给工作区 `read_file` 仍会被拒绝。宿主未提供 SDK 返回 `NotFound`，导出失败返回 `OperationFailed`，未协商返回 `CapabilityUnavailable`。

这两个操作可在活动实例及只读 `LanguageService` 准备钩子中调用。钩子可以关闭本次创建的文件句柄，其余临时句柄在返回时统一撤销；它不能通过发现发起写入、进程或编辑器操作。迁移钩子仍只有私有副本权限。宿主经 `Manager::open_with_resources` 提供不可变 `HostResources`，同一资源快照传入后台安装、设置替换、工作区切换及失败回滚后的实例。

## ui.clipboard 1.0 与 storage.editor 1.0

`EditorOperation::ReadClipboard` / `WriteClipboard { text }` 分别返回 `EditorValue::Clipboard { text }` / `Unit`。它们需要协商 `ui.clipboard` 并批准 `clipboard` 权限，单次文本不超过 1 MiB。操作通过编辑器请求队列执行，`Accepted` 只代表已入队；插件应等待对应请求的完成通知，不能将延迟结果应用到已经切换或关闭的目标。

`EditorOperation::OpenDataFile { path }` 需要协商 `storage.editor` 与 `storage` 权限。路径为当前插件私有目录内、使用 `/` 的相对文件路径，不允许 `..`、绝对路径或越界链接；成功返回 `Unit`，文件进入宿主正常文档生命周期。它不允许打开其他插件或任意本机文件。

这三种操作当前仅在活动工作区实例可用，遵循统一超时、取消与实例撤销规则。原生宿主在执行副作用前检查请求状态；取消不意味着回滚已经完成的剪贴板写入或文档打开。

## configuration 1.0

协议 7 清单的 `settings` 以插件内部键声明 `title`、`value_type`、`default`、`scope` 和 `apply`。类型为 boolean、string（max_length）、integer（min/max）、enum（choices）；作用域为 user 或 project，后者允许明确确认的项目覆盖。此版本的生效方式为 `restart_instance`。最多 64 个字段、字符串最多 4096 字节、枚举最多 32 个不重复选项；无效默认值在包检查时拒绝。可执行包需声明必需能力 `configuration: ^1`。

可选 `settings_hook: true` 接收 `Notification::Configuration { phase: Validate, values }`，通过 `Output.configuration: Proposal` 返回 discovered 和 errors。钩子在候选实例的准备阶段运行，继承有限指令与内存预算，不能获取活动实例资源或授权；读取已获权限的包资源仍可用。发现值只填充未显式设置的项，任何错误使应用失败。随后 Apply 阶段收到带 Source 的最终值，供 Activate 使用；此阶段失败也不替换旧实例。配置被限制在插件命名空间，访客不能经配置获得权限或修改宿主用户设置。

用户设置独立于访客私有文件，已确认项目值保存在宿主管理的工作区记录，不自动信任仓库文件。热应用、初始化与再次打开工作区使用相同解析路径。应用失败保留先前配置和运行实例；成功只替换相关实例并撤销其旧资源。

## 独立构建与 SDK 分发

本目录是主程序持有的版本化插件接口。`wit/plugin.wit` 定义 WebAssembly Component Model 的导入与导出；Rust crate `plugin-protocol` 定义通过该接口传递的 JSON 消息、文档和权限名称。主程序把这些接口文件编入可执行文件，并自动管理插件编译所需的接口缓存。

`host.request` 是主程序在运行时提供的导入。当前插件通过 `api::guest` 使用类型化能力请求，主程序逐次检查协商能力、插件权限、实例作用域和句柄。WIT 和 Rust 类型只在编译插件时使用，安装后的 `.wasm` 不读取 SDK 文件。

插件在 `Cargo.toml` 声明 `plugin-protocol = { version = "=0.2.0", features = ["guest"] }`，通过 `plugin_protocol::bindings::{Guest, editor, export}` 使用宿主调用和组件导出，无需自行生成 WIT 绑定。独立编译时直接调用已打包的编辑器：

```powershell
editor-app.exe --plugin-cargo capability-example/Cargo.toml build --target wasm32-wasip2 --release
# 同一入口也支持原生单元测试和编译检查。
editor-app.exe --plugin-cargo capability-example/Cargo.toml test --lib
editor-app.exe --plugin-cargo capability-example/Cargo.toml check --target wasm32-wasip2
```

编辑器将内嵌接口按内容摘要缓存到系统用户缓存目录的 `MeEditor/plugin-sdk/<摘要>/`（Windows 为 `%LOCALAPPDATA%/MeEditor/plugin-sdk/<摘要>/`），通过本次 Cargo 命令的依赖覆盖选择该缓存。不同接口版本不会互相覆盖；缓存缺失或损坏会自动恢复。插件项目不需要 `sdk/`，也不引用主程序源码；发行目录只需主程序与插件包。接口变更时仍需核对 WIT 包版本和插件清单的 `protocol` 版本。

开发机器需安装 Rust/Cargo 与 `wasm32-wasip2` 目标；使用已编译插件的用户无需这些工具。`--plugin-cargo` 执行开发者提供的 Cargo 项目，属于本机开发工具，不是运行时沙箱。为其他语言工具链或接口检查保留显式 `--export-plugin-sdk <目录>`，常规构建与发行不调用它。

在本编辑器内开发插件时，Rust 插件通过 `host.sdk` 获取与 `--plugin-cargo` 相同的宿主 SDK 缓存，通过 `workspace.files` 发现独立插件项目。插件的 WASM 钩子生成 Rust Analyzer 的 `cargo.configPath`、`linkedProjects` 和配置节；宿主按通用 LSP 协议传递这些数据。项目发现遵循忽略规则，并由 Rust 插件声明排除构建产物和 vendor 目录。输入 `plugin_protocol::api::` 可以获取类型补全、悬浮说明和定义跳转。插件目录无需增加 SDK、Cargo 配置或指向宿主源码的路径；定义跳转会打开宿主缓存中的协议源码。需要已启用 Rust 语言插件并安装 Rust Analyzer。

## process 1.1 与 language.lsp 1.1

原生服务可声明 `search_paths` 与 `check_args`。前者最多 32 个绝对路径 glob，唯一支持的变量为 `${HOME}/` 前缀；不得包含 `..`、控制字符或递归 `**`。按声明顺序搜索，同一模式的候选按路径倒序排列，然后查找绝对 PATH 目录。搜索最多访问 20,000 个目录条目，中间候选和发现结果各最多 256 个，整个发现最多五秒；访客请求同时受本次调用的更短期限约束。`check_args` 作为字面参数数组运行，不使用 Shell；探测最多两秒并共享发现剩余期限，退出失败的候选被跳过。所有探测使用宿主进程所有权和清理规则。

服务 `program` 或 `executable_setting` 的显式绝对路径只有一个候选，错误会直接呈现，不回退到搜索结果。安装服务权限批准该声明的查找和探测行为；这些字段不授予动态可执行程序权限。

LSP 提供者可声明 `client_experimental`，传入 `initialize.capabilities.experimental`；标准传输能力仍由宿主维护。最多 64 个非空键，每键最多 256 字节，JSON 深度最多 16，且包含在提供者总计 256 KiB 的限制内。宿主不识别具体服务名称、扩展字段或 Rust 项目结构。

## 当前版本与迁移边界

Rust SDK 0.2.0 删除旧 Message/Event/Reply/Request、Scene/Widget 与 CanvasControls 类型和兼容转换。重新编译时依赖 `plugin-protocol = { version = "=0.2.0", features = ["guest"] }`。当前能力协议的线上格式不变：清单 protocol 7、api.base ^1；WIT 仍是 editor:plugin/plugin@0.1.0，接口能力分别协商版本。

插件只使用 api::Input、api::Notification、api::Output 和 api::Operation；UI 返回 ui::Document，其中 Canvas、SideTabs 是普通节点。宿主嵌入端通过 Manager::event(plugin_id, panel, notification) 路由，读取 Instance::views 获取每个面板的已验证文档，不再经过旧场景转换。

旧协议 1–6 的包拒绝安装和执行，保留安装记录以展示更新/卸载与启用偏好。仅保留有限的历史 ID、私有数据与布局导入；数据导入不会恢复旧代码执行能力。热更新和迁移详情见 [MIGRATION.md](/zh-cn/sdk/migration/)。
[语义视口](/zh-cn/sdk/viewport/)、[链接与导航](/zh-cn/sdk/navigation/)和[只读代码高亮](/zh-cn/sdk/code-highlighting/)增加版本化原生预览交互，不建立另一份可变文档。
