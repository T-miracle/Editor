# 插件平台 API 通用化与可扩展性重构方案

日期：2026-10-02

状态：已完成（2026-10-03）。已确认方案对应的 20 张实施工单均已完成实现、验证、独立双轴审查、提交、推送与关闭。

交付范围：本轮插件平台能力协议、迁移与契约验收已交付，最终提交 `92cca291735a7940990307cea6b9b498d5a46433`。Windows 验收通过；macOS/Linux 未实测或交叉构建，未发布 GitHub Release。详见[最终契约验收](../verification/plugin-api-contract-verification.md)。正文的问题分析和实施顺序保留为设计时的历史基线。

议题：[设计基线 #1](https://github.com/T-miracle/Editor/issues/1)，标签：`ready-for-agent`。

## Problem Statement

编辑器用户希望安装插件后即可使用，新增语言或替换插件不应要求升级或重启主程序。插件开发者希望通过稳定、公开、可组合的接口实现功能，不必了解宿主内部实现，也不应依赖特定内置插件。

当前插件平台已具备 WASM Component 运行、声明式资源包、包管理、权限检查、面板与命令贡献、原生 UI 和画布等基础能力。问题不是从零建立插件机制，而是现有通用机制与历史上为具体需求编写的处理仍然混在一起：

1. 语言加载及状态跟踪存在固定语言集合，新增语言不能完全沿统一声明入口接入。
2. 宿主直接生成 Rust / rust-analyzer 的初始化选项和配置节，语言业务逻辑泄漏到宿主。
3. 编辑器操作使用字符串命令和不统一的返回约定，参数、失败、完成、取消及订阅生命周期不够明确。
4. UI 协议部分字段偏向字符网格；原生组件与画布尚需形成统一的组合契约。
5. 单一协议版本承载不同能力的演进，兼容关系难以按实际使用的能力描述。
6. 热更新、私有数据、原生进程、工作区作用域、权限和插件协作需要统一的资源归属与失败恢复约定。

仅让每个插件都能调用同一入口，不能证明 API 泛用；仅删除某个插件 ID，也不能证明领域逻辑已经移出宿主。

## Solution

建立以公开能力契约为边界的插件平台：宿主提供运行环境、通用能力、原生交互和资源治理；插件提供领域声明、配置计算、业务逻辑与自身数据迁移。

**硬性原则：在宿主已经提供的能力范围内，新增、替换、停用和移除插件，不需要修改宿主代码。宿主不得依据具体插件 ID、语言名称或服务器名称执行专属业务分支。只有新增一种基础能力时，才扩展宿主 API。**

内置插件与第三方插件使用同一套公开接口。纯声明式插件不必附带无业务用途的 WASM 生命周期组件；需要动态行为时才提供 WASM 钩子。

用户安装插件后，宿主完成兼容性、信任与权限检查，准备已获授权的服务依赖，注册贡献，并按需启动服务。已打开文档随之更新，不要求重新打开文档或重启编辑器。首次下载、服务启动和索引允许耗时，界面应区分安装完成、准备中、启动中、可用及失败。

### 领域术语

| 术语 | 本方案中的含义 |
| --- | --- |
| 宿主（Host） | 编辑器提供的插件运行、能力管理和原生集成环境 |
| 插件包（Package） | 可安装的清单、资源和可选 WASM 组件集合；包版本与接口版本不同 |
| 贡献（Contribution） | 插件声明的语言、grammar、LSP、命令、面板、主题、图标等能力描述 |
| 能力接口（Capability interface） | 宿主公开的类型化接口，具有独立版本和权限规则 |
| 提供者（Provider） | 在某一作用域内提供特定能力的插件贡献 |
| 插件实例（Instance） | 某插件在工作区或应用作用域中的运行实体，拥有可回收资源 |
| WASM 钩子（Hook） | 有明确输入输出、权限和执行预算的可选动态扩展点 |
| 服务契约（Service contract） | 插件之间协作时公开的类型、版本和调用语义 |
| 服务依赖（Service dependency） | LSP 可执行程序及其所需运行时，不等同于整个项目工具链 |
| 资源句柄（Resource handle） | 由宿主管理、绑定实例与作用域的进程、任务、面板或订阅引用 |
| 文档 revision | 用于校验异步语言结果是否仍对应当前文档版本的标识 |
| 状态快照（Snapshot） | 插件明确支持恢复的逻辑状态，不是进程或网络连接的内存镜像 |

### 责任边界

| 宿主负责 | 插件负责 |
| --- | --- |
| 包验证、安装事务、权限和兼容性协商 | 声明贡献、所需能力、权限与用途 |
| 依赖下载、校验、缓存、安装与清理 | 声明依赖或计算结构化安装方案 |
| 通用语言注册、文档同步、LSP 通信与生命周期 | 文件匹配、grammar、语言服务定义和动态配置 |
| UI 组件行为、主题集成、绘制及事件分发 | 界面描述、领域绘制内容和业务交互 |
| 请求路由、资源归属、故障处理和日志 | 处理调用结果、恢复自身逻辑状态 |
| 数据副本、事务提交和失败恢复 | 解释数据格式、实现数据迁移规则 |
| 服务发现、提供者选择、请求转发 | 实现服务契约，声明需要的服务 |

## User Stories

### 安装、热生效与可替换性

1. As an editor user, I want to install a language plugin without restarting the editor, so that I can use it immediately in my current session.
2. As an editor user, I want already-open documents to acquire newly installed language support, so that I do not need to reopen files.
3. As an editor user, I want installation, dependency preparation, service startup and readiness to have distinct states, so that I can understand progress and failures.
4. As an editor user, I want to enable, disable and uninstall plugins without restarting the editor, so that I can manage features during work.
5. As an editor user, I want failed update preparation to leave the current version running, so that an unsuccessful update does not interrupt my work.
6. As an editor user, I want failed activation to restore the previous plugin version and managed data, so that I can recover without restarting the editor.
7. As an editor user, I want a successful update to release obsolete resources, so that old services and panels do not remain active.
8. As a plugin developer, I want to add a previously unknown language using only a plugin package, so that no host code changes are required.
9. As a plugin developer, I want built-in and third-party plugins to use the same public interfaces, so that third-party implementations can replace built-in ones.
10. As a plugin developer, I want resource-only packages to remain declarative, so that I do not need an empty executable component.

### 语言、LSP 与依赖

11. As a language plugin developer, I want to declare language recognition, grammar and language-server contributions, so that ordinary integrations need no custom executable logic.
12. As a language plugin developer, I want optional WASM hooks for project discovery and service configuration, so that dynamic requirements stay outside the host.
13. As a language plugin developer, I want the host to manage LSP communication and document synchronization, so that I do not duplicate protocol infrastructure.
14. As an editor user, I want language recognition, highlighting and LSP providers to be selectable independently, so that I can combine suitable implementations.
15. As an editor user, I want an existing provider choice to survive installation of another provider, so that behavior does not depend on installation order.
16. As an editor user, I want a sole remaining provider to take over when the selected provider is removed, so that support continues where the choice is unambiguous.
17. As an editor user, I want to choose between multiple remaining providers, so that the host does not silently make an arbitrary selection.
18. As an editor user, I want authorized service dependencies to be prepared automatically, so that I do not manually install every language server.
19. As an editor user, I want bundled dependencies and explicitly configured local executables to be supported, so that I can use offline or managed environments.
20. As an editor user, I want dependencies installed in private versioned directories, so that plugin installation does not modify my global environment.
21. As an editor user, I want missing compilers and project SDKs to require my explicit choice before installation, so that a language plugin does not silently replace my toolchain.
22. As an editor user, I want dependency download and verification failures to be visible and recoverable, so that a failed setup does not appear successful.

### API、兼容与插件协作

23. As a plugin developer, I want typed operation arguments, results and errors, so that I do not encode operations in ad hoc command strings.
24. As a plugin developer, I want the SDK to correlate requests and responses, so that I do not manually manage transport bookkeeping.
25. As an editor user, I want accepted work to be distinguished from completed work, so that progress indicators reflect actual outcomes.
26. As a plugin developer, I want long-running requests to support deadlines and cancellation, so that work can stop predictably.
27. As a plugin developer, I want each operation to define cancellation semantics, so that I do not mistake cancellation for rollback.
28. As a plugin developer, I want subscriptions with disposable handles and bounded queues, so that event streams cannot accumulate without limit.
29. As a plugin developer, I want capability interfaces to evolve independently, so that unrelated host changes do not force a plugin rebuild.
30. As a plugin developer, I want required and optional capabilities to be negotiated explicitly, so that missing optional functionality can degrade gracefully.
31. As an editor user, I want incompatible packages rejected before activation with a clear reason, so that failure does not occur after partial startup.
32. As a plugin developer, I want to consume versioned services without hardcoding a provider ID, so that another compatible plugin can replace the implementation.
33. As a plugin developer, I want service-provider loss to produce an explicit result, so that callers do not retain invalid references.
34. As an editor user, I want cross-plugin calls to preserve permission boundaries, so that one plugin cannot borrow another plugin's privileges implicitly.

### UI、实例与资源

35. As a plugin developer, I want standard native components with host-managed focus and input methods, so that my UI behaves consistently with the editor.
36. As a plugin developer, I want a custom drawing surface, so that I can implement interfaces such as terminals and visual previews.
37. As a plugin developer, I want native components and drawing surfaces to coexist in one interface, so that I can combine a standard toolbar with custom content.
38. As a plugin developer, I want character-grid metrics to be optional, so that unrelated UI plugins do not depend on terminal-oriented concepts.
39. As an editor user, I want plugin runtime state isolated by workspace, so that one project's configuration does not affect another project.
40. As a plugin developer, I want application-scoped execution to be declared explicitly, so that globally running features have a clear lifecycle.
41. As an editor user, I want disabling an instance to release its tasks, processes, subscriptions and panels, so that no orphaned activity or empty UI remains.
42. As an editor user, I want packages and service downloads to be reusable across workspaces, so that runtime isolation does not require duplicate downloads.

### 权限、故障与可观测性

43. As an editor user, I want required and optional permissions explained together during installation, so that I can make an informed choice without repeated prompts during normal use.
44. As an editor user, I want updates that add permissions to require confirmation, so that a previously trusted package cannot silently gain access.
45. As an editor user, I want denial of new update permissions to keep the old version available, so that refusal does not remove working functionality.
46. As an editor user, I want starting a declared service distinguished from executing arbitrary commands, so that narrow integrations need not receive terminal-level privileges.
47. As an editor user, I want untrusted workspace settings prevented from authorizing execution, so that opening a repository cannot grant itself permissions.
48. As an editor user, I want plugin time, memory and event budgets enforced, so that faulty plugins do not monopolize the editor.
49. As an editor user, I want limited service retries and a manual restart action, so that repeated failure does not create an endless restart loop.
50. As an editor user, I want errors and logs attributed to the failing plugin and operation, so that I can diagnose failures without restarting the host.

### 数据、配置与迁移

51. As a plugin developer, I want to own the interpretation and migration of private data, so that the host needs no plugin-specific schema logic.
52. As an editor user, I want migrations performed on isolated copies before committing, so that a failed update does not corrupt existing data.
53. As an editor user, I want plugin data and settings preserved during the protocol migration, so that moving to the new platform does not reset my setup.
54. As an editor user, I want explicit project configuration to override global defaults, so that each trusted project can use its chosen provider and service settings.
55. As an editor user, I want explicit configuration errors reported instead of silently substituted, so that the effective setup matches my intent.
56. As an editor user, I want configuration changes applied live or by restarting only the affected service, so that the editor session continues.
57. As a plugin developer, I want declared configuration types and defaults, so that the host can validate settings and present a basic configuration UI.
58. As a platform maintainer, I want independent fixture plugins and fault-injection tests, so that passing existing bundled plugins is not mistaken for API generality.
59. As a platform maintainer, I want host dependency checks to reject plugin-specific business branches, so that future changes do not reintroduce coupling.
60. As a plugin developer, I want documented public contracts and a usable SDK, so that I can build a plugin without relying on host source internals.

## Implementation Decisions

### 1. 平台纯净性与模块边界

- 保留并演进现有职责分层：`plugin-schema` 维护稳定声明式格式；`plugin-protocol` 维护公开契约；`plugin-runtime` 负责组件隔离、资源和生命周期；`editor-app` 负责能力与文档、语言及原生 UI 的集成；插件包负责领域实现。
- 不以本方案为理由为每个能力新增 crate。逻辑模块与公共边界优先；物理拆分需由实现复杂度证明。
- 生产核心不得导入具体插件实现，不得通过插件 ID、语言名称或服务器名称触发专属业务逻辑。通用路由、隔离存储和权限校验可以使用不透明插件 ID。
- 发行清单、示例、测试夹具以及按插件命名空间组织的主题资源允许包含具体名称，但不能由这些名称触发隐藏权限或专属流程。
- SDK 内容导出与缓存是宿主的通用构建能力；将其注入某个语言服务器的配置属于语言插件逻辑。迁移后仍应能通过公开能力完成现有插件开发体验。

### 2. 声明式语言贡献与可选 WASM 钩子

- 从当前安装且在作用域内生效的贡献动态建立语言、grammar 与 LSP 注册，不再以固定语言枚举作为发现、加载或状态跟踪入口。
- 支持一个插件声明多个语言或能力，也支持不同插件分别提供识别、高亮和 LSP。
- 普通贡献完全声明式；项目发现、服务位置解析、启动参数、初始化选项和配置计算可以由可选 WASM 钩子提供。
- 钩子必须有明确输入、输出、作用域、权限与执行预算，不能作为绕过通用能力接口的任意宿主入口。
- 宿主负责标准 LSP 连接、文档同步、请求响应、取消、生命周期及编辑器集成；插件不重复实现整套 LSP 客户端。
- 继续采用现有标准输入输出 JSON-RPC 的 LSP 连接范围，不因接口通用化自动引入远程服务传输。
- 语言服务器初始化配置与配置节名称由插件定义；宿主不生成 rust-analyzer 专属选项。
- 高亮继续来自插件提供的 Tree-sitter WASM grammar，不能用恢复宿主内建 grammar 来掩盖动态注册问题。
- 语言能力的安装、切换和撤销必须作用于已打开文档。过期文档 revision、旧实例和旧贡献版本产生的结果不得写回当前视图。

### 3. 提供者选择

- 语言识别、高亮和 LSP 分别选择提供者。默认每份文档使用一个高亮提供者和一个 LSP 提供者。
- 只有一个候选时自动选择；增加竞争提供者时保持已有有效选择，不按安装顺序覆盖。
- 用户全局选择可以被用户确认的项目选择覆盖，内置插件没有隐藏优先级。
- 当前提供者禁用或卸载后，仅一个候选时自动接替；多个候选时提示选择；没有候选时明确降级。
- 多 LSP 同时运行及结果合并不在本次范围。

### 4. 服务依赖与工具链

- 插件声明来源、版本、平台和校验信息；需要动态解析时由 WASM 返回结构化方案，宿主执行通用下载、校验、解包和安装步骤。
- 允许插件包携带服务，也允许用户显式指定本机服务程序。
- 服务与必要运行时默认进入宿主管理的私有版本目录；不修改全局 PATH 或其他全局环境。
- 经安装流程授权后可以自动准备声明依赖；这不授权后台自动升级任意插件或工具。
- 执行原生安装程序或脚本属于单独权限。大型编译器和项目 SDK 缺失时提示用户，只有用户主动选择后才安装。
- 保留通用工具链解析边界，以可执行路径与参数数组启动受管服务，不能为某个服务拼接专属 Shell 命令。
- 缓存清理不能破坏仍被实例使用或更新恢复所需的依赖版本；原生进程与依赖文件分别管理生命周期。
- 支持离线可用来源；下载失败、校验失败、取消和缺失依赖均需产生可诊断状态。

### 5. 热生命周期与两阶段更新

| 阶段 | 行为与不变量 |
| --- | --- |
| 准备 | 旧版本继续服务；验证兼容性、权限、包内容及状态兼容性，准备依赖；新版本不得修改正式数据或抢占贡献 |
| 切换 | 暂停旧版本新请求，获取最终逻辑状态，停止旧服务，切换贡献与实例，激活新版本；允许短暂中断 |
| 提交 | 新版本达到约定的激活成功条件后提交数据和安装记录，释放旧资源 |
| 恢复 | 准备失败不影响旧版本；激活或提交失败恢复旧版本及宿主管理的数据，重启旧服务并同步文档 |

- 安装、启用、禁用、更新和卸载均不得要求重启宿主。
- 准备期间旧版本仍可能修改数据，迁移最终提交必须基于切换时一致的数据版本，不能用过期副本覆盖新写入。具体同步机制属于实现细化。
- 禁用、卸载和实例结束后，拒绝新请求，结清未完成请求，撤销贡献并回收所有归属资源。
- 安装成功不等于所有语言服务已经完成索引。插件激活、服务就绪和服务后台索引应能够独立表达。
- 旧版本恢复也可能遭遇操作系统或服务故障；此时必须明确报告原始失败与恢复失败，保留可恢复数据，不能假报成功。
- 恢复的是逻辑状态和受管数据，不承诺保持原生进程 PID、交互式程序执行进度或网络连接。

### 6. 类型化调用、异步与订阅

- 替换拼接式编辑器命令，用公开操作定义明确参数、返回结果及错误语义。
- 跨边界请求具有统一关联标识，由 SDK 封装；不要求插件作者手动管理传输编号。
- 区分请求受理、进度与最终结果。即时操作可以直接完成，不强迫所有操作产生多次回复。
- 耗时任务支持截止时间、取消及实例退出清理；每种副作用操作明确取消是停止等待、尽力终止还是尚未执行。
- 取消不等于撤销；不能把停止进程描述为恢复其修改的文件。
- 事件订阅返回可释放句柄。事件队列必须有上限，并针对事件语义定义合并、背压或显式失败行为。
- 文档变化可以在保持版本语义的前提下合并为最新状态；终端输出等字节流不能静默丢弃后仍声称数据完整。
- 未知操作、失效句柄、权限不足、超时、服务退出和协议不兼容应有可区分的错误。

### 7. 独立能力版本与 SDK

- 基础协议只承载生命周期、请求关联、取消、错误和能力协商等共同语义。
- 语言、LSP、进程、存储、编辑器操作和 UI 分别定义能力接口版本；包版本不承担接口兼容协商职责。
- 插件声明必需与可选能力。必需能力不可用时激活前拒绝，可选能力不可用时按声明降级。
- 兼容扩展保留既有调用含义；破坏性变更使用新的接口主版本。
- 新能力必须同时交付契约说明、权限规则和行为测试。
- 继续以 WASM Component 为执行边界。采用类型化 WIT 还是类型化消息编码、具体版本区间语法等尚未在访谈中选定，不在本方案冒充已确认决策。
- SDK 为插件提供小而稳定的接口，并保持通过宿主公开构建接口开发的能力；不要求插件引用宿主业务源码。

### 8. 可组合 UI

- 标准原生组件负责布局、基础交互、输入法、焦点与主题集成；插件提供声明和业务事件处理。
- 自定义画布负责插件提供的绘制内容与领域交互；宿主提供通用绘图、尺寸、坐标与输入事件。
- 两者可以在同一界面组合，例如标准工具栏加画布内容，不局限于专门为终端设计的布局槽位。
- 字符网格尺寸、行列概念等按需提供，不能成为所有 UI 插件的基础必填字段。
- 原生控件继续以 gpui-base 行为为基础，通过编辑器自己的 UI 层提供外观，遵守现有主题契约。
- 面板、命令、快捷键及编辑器内预览均通过通用贡献管理，停用和卸载后正确收回空间与事件目标。
- 拥有 UI 能力不自动获得文档、文件、剪贴板或进程权限。

### 9. 作用域与资源归属

- 插件包与依赖缓存可共享，运行状态默认按工作区隔离。
- 应用级声明由宿主统一管理；需要应用级后台执行的插件显式声明应用级实例。
- 进程、请求、任务、订阅、面板与其他资源句柄绑定拥有者和作用域。实例销毁统一回收，不能依赖插件逐个清理成功。
- 工作区级实例只能通过已授权能力访问其所属工作区，跨工作区访问不能隐式发生。
- 用户级配置与项目级数据分开处理。插件包的全局启用默认与某个工作区实例的实际运行状态不是同一个概念。
- 此设计为多工作区保留边界，不将多窗口或多根工作区产品功能加入本次交付。

### 10. 权限、信任与原生执行

- 安装时集中展示必需和可选权限及用途；正常调用不反复确认，但宿主仍逐次检查授权。
- 更新未增加权限时沿用授权；新增权限重新确认，拒绝后保留旧版本。
- 权限尽量带范围：当前工作区读取、自身私有数据写入、启动已声明服务等。
- 启动任意命令是独立权限，不能混同于启动获批的受管服务。两者可以复用通用进程基础设施。
- 权限应绑定所执行的实际资源和允许的操作，不能只凭插件声称“这是某服务”就视为已授权。
- 受限工作区不启动插件和语言工具，不采用项目工具路径；项目文件不能自行授予信任或执行权限。
- 原生程序不自动继承 WASM 沙箱保障。私有安装目录是存储布局，不是操作系统沙箱；界面与文档必须如实说明限制。

### 11. 插件间服务契约

- 插件声明提供或消费的服务契约、参数、结果和版本，宿主负责匹配、路由和生命周期通知。
- 调用方依赖服务而非具体插件 ID；多个提供者由用户选择，缺少服务时明确提示或按可选声明降级。
- 提供者停用、更新和故障后，旧引用失效，未完成调用得到明确结果。
- 插件间调用保留调用来源和授权边界，不能隐式借用提供者的全部权限。
- 本次支持明确的服务调用，不开放插件内部对象或任意共享内存。
- 自定义服务契约的命名、编码、版本区间及调用权限落实方式需在详细契约设计中定义，不将字符串转发当作完成标准。

### 12. 故障处理与可观测性

- 每个实例具有执行时间、内存和队列预算。WASM 钩子超限时终止该次执行，反复失败暂停实例。
- 插件执行与耗时任务不阻塞 UI 线程。
- 原生服务重启次数有限，持续失败停止自动重试，提供日志与手动重启。
- 失败定位到插件、作用域及操作；结清请求和回收失效资源，保证用户文档不丢失。
- 重启预算、具体超时和配额数值未在访谈确定，应作为可测试的策略参数在实现阶段制定。
- 不承诺通过 WASM 和普通原生进程隔离消除一切宿主崩溃或系统资源风险；需要针对可控制的行为分别验证。

### 13. 数据迁移与配置优先级

- 插件声明数据格式版本，并按需提供迁移钩子。宿主仅理解版本及事务封装，不解释业务字段。
- 迁移在隔离副本中执行，仅允许操作本次迁移的数据；成功且新版本激活后提交，失败恢复旧数据。
- 项目文件和外部系统不纳入插件私有数据自动回滚。
- 配置优先级为：用户确认的项目配置 → 用户全局配置 → WASM 自动发现值 → 插件声明默认值。
- 插件声明配置类型、默认值与生效方式，宿主据此校验并生成基础设置界面。
- 动态钩子可以报告显式配置无效，不得悄悄替换用户指定的程序或参数。
- 配置尽量热生效，必要时只重启受影响实例或服务。
- 上述项目覆盖适用于允许项目作用域的插件配置和提供者选择，不扩大到编辑器界面语言、主题、信任等用户级设置，也不允许配置覆盖授权。

### 14. 统一迁移到新协议基线

- 同步迁移宿主、SDK、打包流程、文档及仓库内现有插件，不保留永久旧协议运行转换层。
- 当前迁移对象包括终端、示例、SVG 预览及 Rust、TOML、HTML、JavaScript 语言资源包；此清单仅用于本次交付盘点，不成为新的宿主运行白名单。
- 旧协议插件包在激活前明确拒绝并提示更新。已有设置、私有数据、全局启停与项目作用域记录需要独立迁移，不能直接清空。
- 旧安装记录或数据的必要导入迁移应作为有限、可测试的数据升级处理，不成为宿主永久的插件专属执行逻辑。
- 插件清单版本、接口声明与打包产物同步更新；声明式插件继续允许不包含 WASM 组件。
- 现有用户可见能力纳入回归，包括终端输入与尺寸变化、面板隐藏后空间回收、Markdown 插件说明、主题、图标、预览与管理操作状态。

### 15. 与现有规范的关系

| 现有约定 | 本次处理 |
| --- | --- |
| 不自动下载、安装工具 | 被本次已确认的“授权后自动准备服务及必要运行时”替代；不扩张为自动安装全部项目工具链或后台自动升级 |
| 插件仅含声明式资源 | 扩展为声明式资源加可选 WASM 行为；原生服务是受控依赖，不作为宿主原生插件模块加载 |
| 权限仅安装时展示 | 扩展为初次安装确认，更新新增权限再次确认；正常调用不重复弹窗 |
| 固定语言集合与宿主专属 LSP 配置 | 移入动态贡献与插件钩子 |
| 旧协议持续兼容 | 本次统一建立新基线；旧包明确拒绝，数据另行迁移 |
| 唯一文档状态、revision 校验、受限工作区、插件 WASM grammar、原生 UI 基座 | 继续遵守 |

这份方案记录已确认的新决策及其覆盖范围，不追溯改写历史需求文档；实施时应同步更新仍用于指导实现的规范与 SDK 文档。没有被本次明确改变的规则继续有效。

## Testing Decisions

### 主要测试边界（已确认）

**优先复用“插件包进入现有管理器，经宿主集成后产生可观察行为”这一条纵向验证路径。**

- 从真实或最小有效插件包开始，通过现有包读取、Manager 安装／启停／更新／卸载入口驱动运行时。
- 使用既有编辑器集成入口把贡献应用到文档、语言服务、命令和界面；通过 GPUI 测试驱动真实文档与交互场景。
- 运行时测试与编辑器集成测试是同一产品流程的不同验证层次，不为每种插件新增专属测试 API。
- 测试优先断言外部行为、状态和资源是否释放，不断言内部映射结构、私有字段、辅助函数调用次数或固定枚举内容。
- 下载源、时钟和语言服务器等不可控外部依赖可以在既有边界替换为本地可控实现；新增测试替换点必须必要且处于较高层，不拆成大量薄接口。
- 纯协议校验、路径约束、版本匹配等适合补充小范围测试，但不能替代完整包流程验证。

### 已有测试先例

1. 真实终端包的运行时 smoke：权限拒绝、PTY 输出、更新失败恢复、成功切换、快照和卸载数据策略。
2. 声明式资源包生命周期 smoke：安装、启停、卸载以及重新打开管理器后的记录恢复。
3. 运行时已有的文件能力边界、无效场景拒绝及初始化失败不能修改原设置测试。
4. GPUI 原生面板、命令参数、输入法、面板范围和隐藏后回收 Dock 空间的集成测试。
5. 编辑器启动和语言状态测试：项目启用、过期服务结果、贡献移除后的文档重置。
6. SVG 预览跟随打开文档及未保存内容的原生集成测试。

这些先例部分依赖具体插件和内部状态，可复用驱动方式与场景，不原样继承实现细节断言作为新的通用性证明。

### 验收矩阵

| 编号 | 场景 | 通过标准 |
| --- | --- | --- |
| T01 | 未知语言声明式包 | 不修改宿主，安装后识别并高亮已打开文档，卸载后撤销 |
| T02 | 未知语言加动态 LSP 钩子 | 使用独立测试服务，能够观察插件生成的启动与初始化配置，宿主无语言专属分支 |
| T03 | 一个包贡献多个语言 | 各语言正确注册、撤销和跟踪失败，不依赖一包一语言假设 |
| T04 | 替换能力提供者 | 同能力不同 ID 的实现可接替；安装顺序不覆盖已有选择；项目选择正确生效 |
| T05 | 提供者移除 | 单一候选自动接替，多候选明确选择，无候选明确降级 |
| T06 | 私有依赖安装 | 下载和校验正确，离线来源可用，不修改全局环境；缓存清理不损坏活跃依赖 |
| T07 | 依赖故障 | 下载失败、校验失败和取消产生明确状态，旧实例不受准备失败影响 |
| T08 | 完整热生命周期 | 已打开文档和界面随安装、启停、更新、卸载变化，宿主无需重启 |
| T09 | 两阶段更新故障 | 在准备、迁移、激活、提交边界注入失败，正确保留或恢复旧版本和数据 |
| T10 | 更新时旧版本继续写数据 | 新版本不提交过期副本，不丢失切换前最终数据 |
| T11 | 迟到异步结果 | 旧文档 revision、旧实例和旧贡献版本结果被丢弃，不污染新状态 |
| T12 | 请求与取消 | 参数、结果、错误和请求关联正确；受理不冒充完成；取消遵守操作约定 |
| T13 | 事件积压 | 队列受限，可合并事件保持最新语义，不可丢数据流遵守背压或显式失败约定 |
| T14 | 能力协商 | 缺少可选能力可降级，缺少必需能力激活前拒绝；不相关能力升级不破坏兼容 |
| T15 | UI 组合 | 独立表单、独立画布、原生组件加画布通过公开协议完成，输入法与焦点正确 |
| T16 | 工作区隔离 | 两个逻辑工作区配置、资源句柄和事件互不越界；不要求新增多窗口产品界面 |
| T17 | 应用级实例 | 显式声明后按应用作用域管理，不随单个工作区错误退出 |
| T18 | 权限变化 | 未授权调用失败；更新新增权限需确认，拒绝后旧版本继续使用 |
| T19 | 原生执行权限 | 声明服务权限不能被用于执行任意程序；受限工作区拒绝执行 |
| T20 | 插件间协作 | 更换提供者后消费者无需修改；缺失与退出明确失败；不能借调用扩大权限 |
| T21 | 故障预算 | WASM 超限被控制，服务有限重试，日志可定位，未完成请求结清 |
| T22 | 资源回收 | 停用或卸载后无遗留进程、订阅、活动任务和空白面板占位 |
| T23 | 配置层级 | 已确认项目值覆盖全局，显式错误不被自动发现掩盖，配置不能授予权限 |
| T24 | 数据与旧包迁移 | 设置及数据按方案保留，旧协议包有明确拒绝原因，不静默启动或清空数据 |
| T25 | 现有插件回归 | 终端、示例、SVG 和语言资源包通过新公开接口保持既有功能 |
| T26 | 架构纯净性 | 公共协议、运行时与通用能力不依赖插件实现；名称只出现在合理的发行、测试或命名空间数据中 |

### 执行约束

- 独立夹具包含宿主从未认识的语言 ID、动态配置服务、不同提供者和 UI 组合，不能只证明已有插件可运行。
- 故障注入使用隔离测试目录与本地服务，不访问或破坏用户已安装插件、实际项目和全局工具链。
- Windows 作为当前主要行为验收平台；其他系统维持既有构建承诺，不将未执行的跨平台场景描述为已验证。
- 实施各阶段按仓库要求完成格式检查、工作区非 UI 测试、工作区编译检查，并执行受影响的 GPUI 与实际包验证。
- 本次仅编写规格，不运行代码测试，也不宣称上述新增验收项已经通过。

## Out of Scope

- 本次文档交付不直接修改生产代码、不发布新版插件包、不提交 Git。
- 更新期间所有功能零中断，以及恢复原生进程、网络连接和外部副作用的完整快照。
- 永久保留旧插件运行协议的兼容转换层；已有用户数据迁移仍在范围内。
- 多个 LSP 同时服务同一文档时的结果合并。
- 插件内部对象共享、任意共享内存或无权限约束的插件间调用。
- 操作系统级完整沙箱及所有原生程序副作用的隔离保证。
- 自动安装所有项目编译器、SDK，或静默升级插件和工具。
- 新增远程 LSP 传输、多窗口、多根工作区、在线市场等无关产品功能。
- 借通用化之名新增不需要的语言业务功能，或为未来假想需求扩展所有宿主能力。

## Further Notes

### 建议实施顺序

| 阶段 | 交付内容 | 阶段完成证据 |
| --- | --- | --- |
| 1. 契约与基线 | 公共能力边界、声明结构、错误与生命周期约定、旧包迁移盘点、独立测试夹具设计 | 契约可评审，现有功能和新增验收项有对应关系 |
| 2. 运行时基础 | 能力协商、请求生命周期、作用域、权限、资源归属与执行预算 | 通用夹具证明资源和权限边界，不依赖具体插件 |
| 3. 语言与服务 | 动态贡献、提供者选择、LSP 钩子、工具链与依赖管理 | 未知语言包安装后使已打开文档获得高亮和测试 LSP |
| 4. UI 与协作 | 组合 UI、类型化编辑器操作、插件服务契约 | 标准组件和画布组合正常，服务提供者可替换 |
| 5. 热事务与迁移 | 最终热切换、数据事务、配置层级、故障恢复、现有插件迁移 | 更新失败恢复与用户数据保留通过验证 |
| 6. 收尾交付 | SDK、打包与使用文档更新，旧协议和宿主专属分支移除，完整回归 | 验收矩阵通过且没有未披露的恢复或兼容缺口 |

各阶段建立纵向可验证场景；生命周期和权限约束从阶段 1 即进入设计，不能等到阶段 5 才补充。阶段拆分不授权将未迁移的中间状态当作最终用户发行版。

### 实现前细化项（历史设计记录，已在本轮实施落实）

- 能力标识、类型编码、版本区间、必需与可选能力的清单语法。
- 各钩子的输入输出、就绪判定、并发与重入规则，以及具体执行预算。
- 提供者兼容匹配、依赖／服务循环检测和调用授权传播的具体机制。
- 热切换期间一致性快照、提交持久化顺序及宿主意外退出后的恢复策略。
- 原生运行时的各平台支持矩阵、下载源政策与校验信息表达。
- SDK 构建、打包和旧安装数据升级的具体版本号与发布批次。

这些是落实已确认目标的详细设计工作，不得用于重新引入固定语言列表、专属宿主命令、隐式权限或静默数据丢失。影响已确认产品行为的新取舍需另行评审。

### 发布与审核状态

- 已确认：本规格中的产品原则、架构方向、统一迁移策略和行为验收目标。
- 已确认（2026-10-02）：以现有包管理器加编辑器集成作为主要测试边界；不为具体插件新增专属测试接口。
- 已确认并发布：[20 张垂直切片工单及依赖图](../tickets/README.md)，对应 GitHub #2–#21。
- 已采用 `T-miracle/Editor` GitHub Issues，[跟踪器约定](../../agents/issue-tracker.md)与本次分诊标签已记录。
- 初次发布时设计基线与工单采用 `ready-for-agent` 标签，并核对了 28 条原生阻塞关系；此为发布历史，不代表当前实施状态。
- 已完成（2026-10-03）：工单 01–20 / GitHub #2–#21 全部交付并关闭，最终提交 `92cca29` 已推送。设计基线 #1 保留，不关闭或改写。
