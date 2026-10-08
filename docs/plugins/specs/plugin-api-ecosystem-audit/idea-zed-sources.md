# IntelliJ Platform 与 Zed 插件 API 一手来源记录

调研日期：2026-10-08。状态：调研材料，非实施规格、兼容承诺或发布验收。本文仅记录外部平台事实；Nanobug 的已有能力、缺口与优先级由主报告结合当前协议、运行时和宿主调用链判断。

## 口径与限制

- IntelliJ IDEA 的插件能力应区分 IntelliJ Platform 通用 API、特定产品/语言插件的依赖，以及 Internal、Experimental、Deprecated API。Java 的 `public` 可见性不等于对第三方承诺稳定；官方明确禁止插件使用 Internal API，并提供 Plugin Verifier 检查。[兼容性验证](https://plugins.jetbrains.com/docs/intellij/verifying-plugin-compatibility.html)／[模块依赖](https://plugins.jetbrains.com/docs/intellij/plugin-compatibility.html)
- Zed 的产品功能、GPUI 源代码能力和 WASM 扩展公开接口是不同集合。以下以官方扩展文档及第一方 `zed_extension_api` 的已发布 Rust API 文档为依据；检索时后者显示 **0.7.0**，不将其解释为当前 Zed 应用版本。[扩展开发](https://zed.dev/docs/extensions/developing-extensions)／[SDK 目录](https://docs.rs/zed_extension_api/latest/zed_extension_api/)
- 文档和 `master` 源码 URL 会变化。本次是访问日快照式调查，没有逐版本运行第三方插件，也没有证明任意平台版本间二进制兼容。准备具体实现时仍需选定目标版本、检查符号状态与实际分发 SDK。

## IntelliJ Platform：可供通用 API 设计参考的能力

| 能力族 | 已核实的公开机制与语义 | 对 Nanobug 审计的启发（设计推论） |
| --- | --- | --- |
| 文档读写与撤销 | `Document` 支持文本与变更监听；文档修改除读写规则外还必须置于 command 中，外层 command 进入撤销栈；多文档修改也有撤销语义。[Documents](https://plugins.jetbrains.com/docs/intellij/documents.html) | 不只核对“能改文本”，还要核对多文档事务、撤销分组、脏文档、保存/重载事件和只读保护。 |
| 编辑器实例与事件 | `FileEditorManager`、编辑器上下文、项目级打开/关闭/选中事件、选区和多光标 API 是公开文档的一部分。[Editors](https://plugins.jetbrains.com/docs/intellij/editors.html) | 文档身份、显示实例和当前焦点应分离；检查光标/选择/可见区是否有公开访问与订阅。 |
| 自定义文件显示 | 官方扩展点清单列出 `com.intellij.fileEditorProvider`；相邻的 `fileEditorProviderSuppressor` 标为 Internal，不能一并视为稳定公开 API。[扩展点清单](https://plugins.jetbrains.com/docs/intellij/extension-point-list.html) | 自定义文件编辑器/预览与宿主文件模型衔接应有正式入口，不以任意访问内部布局代替。 |
| 虚拟文件与文件事件 | VFS 为本地、归档、HTTP 等文件提供统一抽象，并暴露批量/异步文件事件；快照刷新与磁盘即时状态不同。[VFS](https://plugins.jetbrains.com/docs/intellij/virtual-file-system.html) | URI/scheme、资源元数据、目录操作、watch、变更一致性应单独审计，不能以本地路径读取代表完整文件能力。 |
| 语法与语义模型 | PSI 是解析文件并构建语法、语义代码模型的层。[PSI](https://plugins.jetbrains.com/docs/intellij/psi.html) | 语法树查询与语言服务是两条通路；是否提供宿主管理的语法查询，应按目标插件类型判断，不必照搬整个 PSI。 |
| 索引与缓存 | 插件可使用平台已有索引或定义 file-based/stub indexes；DumbService 表示索引不可用阶段；gists 提供惰性文件缓存。[Indexing and PSI Stubs](https://plugins.jetbrains.com/docs/intellij/indexing-and-psi-stubs.html) | 项目级查询需要可取消、增量更新、索引未就绪状态和缓存失效规则；这比只提供文件枚举更完整。 |
| 并发、一致性与取消 | PSI/VFS 等模型访问受读写机制保护；官方要求缩短锁持有时间，并提供可取消/重试的非阻塞读取与对象有效性规则。[Threading Model](https://plugins.jetbrains.com/docs/intellij/threading-model.html) | WASM 宿主无需复制 JVM 锁模型，但需要 revision、取消、迟到结果拒绝、UI 线程预算和句柄有效期。 |
| 命令、菜单与快捷键 | Action System 覆盖动作 ID、分组、菜单/工具栏、上下文、显示/启用状态、快捷键和本地化。[Action System](https://plugins.jetbrains.com/docs/intellij/action-system.html) | “有按钮”不等于有命令生态；检查同一命令能否复用到菜单、快捷键、面板及自动化入口。 |
| 工具窗口与生命周期 | `ToolWindowFactory`/`ToolWindowManager` 支持声明与动态注册、延迟创建、内容管理、焦点、显示事件及 Disposable 清理。[Tool Windows](https://plugins.jetbrains.com/docs/intellij/tool-windows.html) | 宿主绘制、插件提供内容的设计可保留；应补齐焦点、键盘、状态保存、显隐和卸载释放等行为。 |
| 普通状态持久化 | 应用/项目 service 可使用 `PersistentStateComponent`；支持状态加载、变更跟踪、存储位置与同步策略。[Persisting State](https://plugins.jetbrains.com/docs/intellij/persisting-state-of-components.html) | 明确全局/工作区/实例状态、迁移、原子保存、配额与同步资格，而非仅给插件一块任意目录。 |
| 凭据存储 | `PasswordSafe` 用于密码、API key 等；官方要求在后台调用，并采用系统相关存储。[Sensitive Data](https://plugins.jetbrains.com/docs/intellij/persisting-sensitive-data.html) | 网络型插件需要独立 secrets API；普通配置/私有 JSON 不应承担安全凭据库职责。 |
| 运行、控制台与进程 | Execution API 组织 RunProfile、ProgramRunner、ExecutionConsole、ProcessHandler；提供停止/重启、输出处理、超链接过滤和执行事件。[Execution](https://plugins.jetbrains.com/docs/intellij/execution.html) | 审计目标、配置、会话、输出、退出与取消的整条链，而不是只检查 `spawn`。 |
| 运行配置 | `ConfigurationType` 和 factory 注册配置类型；配置可由 UI 管理和持久化，包含 cwd/env/args。[Run Configurations](https://plugins.jetbrains.com/docs/intellij/run-configurations.html) | 插件定义业务模板，宿主提供通用表单、校验、存储与执行组合。 |
| 调试 | 官方 `XDebugProcess` 说明允许自定义语言/框架调试，并暴露断点、步进、暂停、恢复、停止和求值编辑器等接缝；同一类中 Drop Frame/替代源码视图等方法标为 Experimental。[XDebugProcess 官方源码](https://raw.githubusercontent.com/JetBrains/intellij-community/master/platform/xdebugger-api/src/com/intellij/xdebugger/XDebugProcess.java) | 调试不是“有/无”二分；逐项区分基础会话、attach、数据断点、内存/反汇编等高级能力，以及适配器是否支持。 |
| 测试结果模型 | `SMTestRunnerConnectionUtil` 官方源码提供把进程连接到测试树、控制台与统计视图的入口，并通过事件处理器及 locator 映射测试位置。[SM Runner 官方源码](https://raw.githubusercontent.com/JetBrains/intellij-community/master/platform/smRunner/src/com/intellij/execution/testframework/sm/SMTestRunnerConnectionUtil.java) | 通用执行/终端不能替代测试发现、测试树、单项结果、源位置、重跑与覆盖率模型；源码入口仍需按目标平台版本核对。 |
| VCS | 通用模块表列出 revision、file status、change lists、history、annotations；插件需声明相应模块依赖。[模块依赖](https://plugins.jetbrains.com/docs/intellij/plugin-compatibility.html) | 要区分宿主内建 Git 功能和插件可调用的通用 SCM Provider/状态/差异接口。 |
| 释放与动态卸载 | 动态插件需满足动态扩展点、资源清理等约束；卸载失败会要求重启，不能宣称任何插件均无条件热卸载。[Dynamic Plugins](https://plugins.jetbrains.com/docs/intellij/dynamic-plugins.html)／[Disposer](https://plugins.jetbrains.com/docs/intellij/disposers.html) | 每种注册项、任务、进程、订阅与 UI 都要有实例归属和撤销路径；热更新需针对真实插件验证。 |
| 兼容与开发工具 | 官方提供 API 状态检查与 Plugin Verifier；API 可能跨发行版产生不兼容变化。集成测试框架还区分启动/配置 IDE 的 Starter 与交互 Driver。[兼容性验证](https://plugins.jetbrains.com/docs/intellij/verifying-plugin-compatibility.html)／[不兼容变更](https://plugins.jetbrains.com/docs/intellij/api-changes-list.html)／[集成测试](https://plugins.jetbrains.com/docs/intellij/integration-tests-intro.html) | 社区可用性还依赖兼容矩阵、开发宿主、契约测试、样例与故障诊断，不能只统计接口名称。 |

### 不宜直接复制的部分

IntelliJ 的可扩展面很广，但其中部分能力依赖特定模块；Internal/Experimental 等状态必须逐符号核对。因而“IDEA 源码里找得到”不能直接作为 Nanobug 上线必须提供该 API 的证据。[模块依赖](https://plugins.jetbrains.com/docs/intellij/plugin-compatibility.html)／[API 状态](https://plugins.jetbrains.com/docs/intellij/verifying-plugin-compatibility.html)

设计推论：Nanobug 可以提供版本化请求、声明式 UI、文档事务、受控工具与服务注册这些通用机制，而不是暴露宿主原始对象、复制 PSI 全套体系，或让插件直接操纵 GPUI 内部状态。是否实施远程开发、多窗口、完整项目索引等新产品能力，应由主方案另行确认。

## Zed：实际的 WASM 扩展面与边界

| 项目 | 访问日核实结果 | 稳定性/解释边界 |
| --- | --- | --- |
| 包与执行模型 | Git 仓库加 `extension.toml`；行为代码使用 Rust→WASM；主题等声明式资源无需必然包含 Rust。[扩展开发](https://zed.dev/docs/extensions/developing-extensions) | `wasm32-wasip2` 扩展不是 GPUI 原生宿主代码。 |
| 语言资源 | Tree-sitter 语法/查询、语言配置、文本对象、runnables 检测、LSP 注册与语义 token 默认映射均有正式文档。[Language Extensions](https://zed.dev/docs/extensions/languages) | `runnables.scm` 的运行按钮检测属于语言资源扩展，不等同于通用测试控制器。 |
| LSP 适配 | `Extension` 提供服务器命令、初始化/工作区配置，以及补全/符号标签钩子。[Extension trait](https://docs.rs/zed_extension_api/latest/zed_extension_api/trait.Extension.html) | LSP 支持什么与插件能直接调用任意编辑器功能是两个问题。 |
| DAP 与目标定位 | 插件可注册 DAP server 和配置 schema；`get_dap_binary`、`dap_request_kind`、`dap_config_to_scenario` 连接启动/附加；debug locator 可把任务转换成调试场景并解析构建产物。[Debugger Extensions](https://zed.dev/docs/extensions/debugger-extensions) | 不能沿用“Zed 扩展没有调试器 API”的旧判断。它支持独立 locator，不要求定位逻辑与 adapter 绑在同一插件。 |
| MCP server | 文档仍说明通过扩展在 Agent Panel 提供 MCP server，同时顶部明确计划弃用这种扩展分发入口，迁向官方 MCP registry。[MCP Server Extensions](https://zed.dev/docs/extensions/mcp-extensions) | 应标记“当前可用、迁移中”，不能推荐其分发入口为长期稳定设计模板。MCP 能力不等于任意插件 UI API。 |
| ACP agent server | 官方页面说明从 Zed v1.5.0 起 ACP extensions 已弃用，转向 ACP Registry。[Agent Server Extensions](https://zed.dev/docs/extensions/agent-servers) | 不列为新的稳定扩展 API；本文仅转述访问日文档，不据此推断所有用户都在该应用版本。 |
| slash commands / 文档索引 | 已发布 SDK 0.7.0 trait 仍列出 `run_slash_command`、参数补全、`suggest_docs_packages`、`index_docs`。[Extension trait](https://docs.rs/zed_extension_api/latest/zed_extension_api/trait.Extension.html) | 证明 SDK 符号存在；本次未实测当前 Agent UI 对这些旧 Assistant 入口的可达性，不能扩大为通用命令注册体系。 |
| 主题、图标、片段 | 官方扩展开发索引明确列出 themes、icon themes、snippets。[扩展开发](https://zed.dev/docs/extensions/developing-extensions) | 是资源贡献入口，不代表插件能够注入任意工具窗口或自定义编辑面板。 |
| 工具与环境 | SDK 公开 HTTP/process/settings 模块、平台查询、文件下载、GitHub release、Node/npm、TCP 模板解析；Worktree 提供读取、PATH 查找与 shell environment 等入口。[SDK 目录](https://docs.rs/zed_extension_api/latest/zed_extension_api/)／[Worktree](https://docs.rs/zed_extension_api/latest/zed_extension_api/struct.Worktree.html) | 这些操作服务于受支持扩展点；不要把 `KeyValueStore` 文档索引句柄直接解释为任意全局 secrets/storage 能力。 |
| 权限 | 官方能力文档列出 `process:exec`、`download_file`、`npm:install`，用户可限制，未授权调用返回错误。[Capabilities](https://zed.dev/docs/extensions/capabilities) | 该页面的能力集合不能被推断为完整 OS 沙箱保证，尤其原生子进程另有边界。 |

### 明确没有据此认定存在的 API

对照本次读取的完整 SDK 目录和 `Extension` trait，**没有找到**通用自定义面板/任意 GPUI UI 注入、任意文档事务编辑、通用命令与菜单注册、SCM Provider、测试控制器或 Notebook Provider 等与 VS Code 对等的公开 WASM 扩展接口。这里是限定在已发布 SDK 调研范围内的缺乏证据判断，不是声称这些产品功能不存在，也不排除后续版本新增。[SDK 目录](https://docs.rs/zed_extension_api/latest/zed_extension_api/)／[Extension trait](https://docs.rs/zed_extension_api/latest/zed_extension_api/trait.Extension.html)

因此，设计推论是：Zed 适合对照 WASM 隔离、声明式语言资源、受控工具准备和 LSP/DAP 适配接缝；其扩展面本身较聚焦，不应作为“社区通用 API 已完整”的上限。IDEA 可用于发现更广的生态需求，但要过滤内部/实验 API 和产品专属模块。两者都不能替代对 Nanobug 当前真实公开链路的逐项检查。

## 验证说明

本文仅修改调研 Markdown；来源为 JetBrains/Zed 官方文档、JetBrains 官方仓库源码以及 Zed 第一方发布 crate 的生成文档。没有运行 Rust 构建、IDE 实例或发行兼容测试；没有改变任何产品规格或工单状态。
