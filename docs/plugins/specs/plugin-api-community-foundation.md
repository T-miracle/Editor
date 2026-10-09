# 通用插件 API 完善：基础社区生态

日期：2026-10-09

稳定标识：`plugin-api-community-foundation`

设计议题：[#92](https://github.com/T-miracle/Nanobug/issues/92)；实施工单 #93–#99，映射见[发布记录](../tickets/plugin-api-community-foundation/publication.json)。

状态：2026-10-09 用户已确认产品范围、七张工单、九条直接阻塞边与测试接缝，并授权连续执行全部工单、创建执行会话及子代理。实施与验证正在推进，实际完成状态以逐单验收及发布记录为准。

目标跟踪器：T-miracle/Nanobug GitHub Issues。按已获授权发布完整方案并应用 `ready-for-agent`，按依赖实施、测试与审查、普通提交、推送并核对关闭实施工单。

## Problem Statement

社区开发者需要通过公开 SDK 实现常见插件，而不必修改 Nanobug 宿主、阅读宿主内部实现或捆绑拥有过宽权限的辅助进程。现有平台已经具备能力协商、WASM 隔离、原生 UI、插件服务、语言接入、受控进程、更新恢复和独立开发运行，但这些基础尚不能覆盖完整的通用文档访问、跨文件修改、工作区文件管理、宿主交互、直接诊断、网络及安全密钥存储。

用户希望代码生成、检查修复、书签、覆盖率、数据库树、云查询等插件能够可靠工作。当前“宿主自己有功能”与“第三方有正式接口”仍存在差距。自建面板不能替代编辑器内的诊断与装饰，私有文件写入不能替代工作区写入，宿主消息界面不能替代插件通知接口。

首次公开后的接口还需要兼容承诺。不能为了追求接口数量提前稳定尚未验证的契约，也不能让已依赖稳定接口的社区插件被无关升级破坏。

## Solution

首发覆盖基础社区生态：语言、检查与修复、代码生成、通用树视图、编辑器装饰、只读虚拟文档和云服务查询。采用“稳定核心＋实验扩展”，成熟基础能力形成稳定契约，新增直接语言提供者先作为实验能力验证。AI 功能暂不实现。

插件提供业务数据和策略，宿主统一处理权限、资源身份、文档版本、编辑事务、原生呈现、生命周期与清理。所有接口面向全部插件开放，内置和第三方插件使用相同协商与权限机制。

多文件修改先让用户预览确认。已打开文档的修改保留在编辑区，未打开文件的修改写入磁盘。宿主提供当前会话内的整组撤销，并在修改或撤销之前检查冲突。工作区外文件仅能通过用户明确选择取得限定访问权。云服务首发使用安全存储的 Token/API Key，不要求浏览器登录。

## User Stories

1. As a plugin developer, I want to enumerate open documents, so that I can select the intended document without relying on the active tab.
2. As a plugin developer, I want immutable document snapshots and range reads, so that I can inspect unsaved content without maintaining another mutable text model.
3. As a plugin developer, I want document identity, revision and dirty state, so that I can distinguish resources and detect stale work.
4. As a plugin developer, I want document language, encoding and line-ending metadata, so that I can interpret text correctly.
5. As a plugin developer, I want open, close, change and save events, so that my results follow the document lifecycle.
6. As a plugin developer, I want active-editor, selection and visible-range events, so that context-sensitive features follow user actions.
7. As a plugin developer, I want stable resource identities beyond workspace-relative paths, so that generated and historical content can use the same navigation model.
8. As an editor user, I want stale or closed-document results rejected, so that delayed plugin work cannot modify unrelated content.
9. As a plugin developer, I want multiple edits in one document transaction, so that one operation has coherent editing and undo behavior.
10. As a plugin developer, I want a cross-file edit plan including resource operations, so that refactoring and code generation use one public workflow.
11. As an editor user, I want to preview and confirm bulk changes, so that a plugin cannot silently replace my files.
12. As an editor user, I want edits to open documents to remain unsaved, so that I retain control of saving my existing work.
13. As an editor user, I want approved changes to unopened files written to disk, so that bulk operations do not open every file.
14. As an editor user, I want revision and disk-version conflicts to stop stale changes, so that concurrent edits are preserved.
15. As an editor user, I want explicit partial-failure and recovery results, so that I know what actually changed.
16. As an editor user, I want one explicit action to undo a batch in the current session, so that I can recover from an unsuitable result.
17. As an editor user, I want batch undo to stop when any target has later changes, so that recovery cannot overwrite subsequent work.
18. As an editor user, I want ordinary document undo to remain local, so that editing one file does not unexpectedly change another file.
19. As a plugin developer, I want workspace metadata and directory enumeration, so that I can discover resources without guessing paths.
20. As a plugin developer, I want authorized create, write, copy, rename and delete operations, so that scaffolding and conversion tools do not require native helpers.
21. As a plugin developer, I want file watching and bounded discovery results, so that unopened-file changes and large workspaces remain manageable.
22. As an editor user, I want to select an external file or directory explicitly, so that import and export can work without granting access to my whole disk.
23. As an editor user, I want external-file authorization limited to the selected target and operation, so that choosing one file does not authorize unrelated access.
24. As a plugin developer, I want readonly virtual documents, so that I can show historical or generated content without temporary disk files.
25. As an editor user, I want to compare virtual content with a current file, so that I can inspect historical and proposed differences.
26. As a plugin developer, I want typed command invocation and results, so that commands can be composed without ad hoc transport conventions.
27. As a plugin developer, I want menu locations and context-dependent visibility and enablement, so that my actions appear where they are useful.
28. As an editor user, I want plugin commands removed when their instance is disabled, so that unavailable actions do not remain visible.
29. As a plugin developer, I want quick picks, input and file or directory dialogs, so that simple interactions do not require a custom panel.
30. As an editor user, I want attributable notifications, confirmations and cancellable progress, so that background operations remain understandable and controllable.
31. As a plugin developer, I want to publish diagnostics without an LSP server, so that lightweight checkers can use the native problems interface.
32. As an editor user, I want multiple diagnostic sources shown with their origins, so that language errors and independent checks can coexist.
33. As a plugin developer, I want ownership of my diagnostic collection, so that another plugin cannot replace or clear my results.
34. As an editor user, I want diagnostic fixes to use the shared edit workflow, so that fixes obey the same conflict and preview rules.
35. As a plugin developer, I want direct language providers alongside LSP integration, so that simple language features do not require a separate server.
36. As an editor user, I want references, code actions and signature help, so that common language tools have a complete native interaction path.
37. As an editor user, I want existing completion, formatting, rename and outline behavior preserved, so that API expansion does not regress language support.
38. As a plugin developer, I want lazy and paged tree nodes, so that large Todo and database trees remain responsive.
39. As an editor user, I want tree refresh, selection, node menus and keyboard navigation, so that plugin trees behave consistently.
40. As a plugin developer, I want gutter icons, text-range marks and hover descriptions, so that bookmarks and coverage can appear in the editor.
41. As an editor user, I want decoration locations to follow edits, so that markers do not silently point to the wrong content.
42. As an editor user, I want plugin UI to respect theme, language, focus, IME and scaling, so that it integrates with the native editor.
43. As a plugin developer, I want general HTTP requests and streaming responses, so that cloud queries do not require an auxiliary process.
44. As a plugin developer, I want cancellation, proxy support and bounded network consumption, so that slow or large responses remain controlled.
45. As an editor user, I want to supply a Token or API Key through an appropriate interaction, so that I can connect a service without browser login.
46. As an editor user, I want secrets stored separately from settings, logs and ordinary snapshots, so that routine persistence does not expose credentials.
47. As a plugin developer, I want isolated secret read, update and delete operations, so that credentials have an explicit lifecycle.
48. As an editor user, I want plugins unable to read another plugin's private data or secrets, so that installing a plugin does not expose existing accounts.
49. As a plugin developer, I want permission-checked service calls, so that plugins can cooperate without transferring credentials or borrowing privileges.
50. As a plugin developer, I want private directory and cache management, so that long-lived plugins can maintain and clean their own data.
51. As a plugin developer, I want structured configuration and change events, so that settings can be validated and applied predictably.
52. As a plugin developer, I want disposable timers, so that refresh and debounce work can be scheduled without leaking resources.
53. As an editor user, I want plugins activated by commands, languages, views or service demand, so that unused plugins do not all start immediately.
54. As a plugin developer, I want to declare genuine background work, so that continuous features remain possible within explicit limits.
55. As an editor user, I want disabling a plugin to release its processes, timers, subscriptions and contributions, so that no orphaned activity remains.
56. As a plugin developer, I want bounded event streams with explicit loss and recovery semantics, so that overload cannot silently corrupt my view of state.
57. As a plugin developer, I want stable and experimental capabilities clearly distinguished, so that I can choose my compatibility risk.
58. As a plugin developer, I want required and optional capability negotiation, so that unsupported features can be diagnosed or degraded deliberately.
59. As a plugin developer, I want complete SDK types, errors, cancellation and disposal guidance, so that I can build without reading host internals.
60. As a plugin developer, I want an isolated development host and reload workflow, so that testing does not damage my normal settings or plugin data.
61. As a platform maintainer, I want unrelated real plugins to exercise the same interfaces, so that bundled-plugin success does not hide special-case APIs.
62. As an editor user, I want failures, permission denial and unsupported-platform behavior explained, so that unavailable functionality does not look like success.
63. As an editor user, I want update preparation and activation failures to preserve the working version and managed data, so that new interfaces retain existing recovery guarantees.
64. As a platform maintainer, I want compatibility and resource-cleanup checks across the promised release matrix, so that stable APIs are supported by observable evidence.

## Implementation Decisions

### 1. 首发范围与已有基础

- 首发定位为基础社区生态，不追求与任一编辑器全部接口数量对齐。稳定核心覆盖资源、文档、事务、文件、交互、基础诊断、基础 UI 扩展、网络、密钥及生命周期；稳定交付以真实消费者验证为前提。
- 继续复用声明式格式、公开协议与 SDK、插件运行时、编辑器核心、Windows 适配和 GPUI 应用各自职责。不按能力数量新增 crate，不增加仅转发调用的包装层。
- 当前主分支已包含纯补全、独立格式化、单文档 Rename/关联编辑、结构/大纲/折叠。相关能力扩展基于既有消费者，不重复开发旧审计中“分支已有”的内容。
- 独立插件打包、隔离开发运行、重载及失败保留已有实现。本方案完善其新能力覆盖和验收，不将它们当作全新系统。
- 宿主消息已有原生基础，仍需正式的插件调用、权限、实例归属及取消契约。宿主 UI 存在不等于公开能力已贯通。

### 2. 资源、文档和事件

- 统一资源身份能表达本地与只读虚拟资源；本地路径转换与读写能力显式提供。资源标识不是访问授权，不能绕过现有路径与实例边界检查。
- 完善文档枚举、打开、不可变快照、范围读取、dirty、语言、编码与 EOL 等元数据。未保存内容来自唯一文档会话，不从磁盘冒充当前内容。
- 完善打开、关闭、内容变化、保存前后、活动文档、选区和可见范围事件。异步结果必须携带目标和 revision；取消、实例退役、文档关闭或版本过期时不得应用。
- 统一订阅释放、有界队列、顺序、合并、溢出通知与补读语义。坐标单位及 UTF-8/UTF-16、CRLF、非 BMP 字符转换须明确，不允许消费者各自猜测。

### 3. 编辑事务、文件提交与整组撤销

- 单文档支持多范围事务。跨文件操作使用统一编辑计划，包括文本修改、文件创建、改名与删除；修改前进行权限、目标、revision、磁盘版本和冲突预检。
- 批量修改必须预览并由用户确认。已打开文本通过 DocumentSession 修改并保持未保存；未打开文件及资源操作按批准计划提交到磁盘。不得通过文件 API 绕过打开文档的未保存内容和 Undo。
- 预览后目标再次变化时停止应用过期计划，重新生成或重新确认。开始执行后的故障必须报告已完成、失败及可恢复部分，不承诺所有磁盘故障下跨文件原子回滚。
- 当前会话内把一次批量操作记录为一组，提供明确的“撤销此次批量修改”入口。撤销前检查全部目标；无冲突时恢复本次操作改变的内容与文件状态，有后续修改时停止整组自动撤销并列出冲突。
- 普通编辑区 Undo 保持文档局部语义，不隐式修改其他文件。整组协调信息不能成为第二套可变文本或文档撤销栈；涉及内存文本的修改与恢复始终经 DocumentSession/EditorState。
- 首发不承诺跨重启保留整组撤销记录。关闭编辑器后的恢复沿用本地历史能力，不宣称历史可以恢复所有外部副作用或全部批次状态。
- 取消不等于回滚；提交与撤销过程中发生 I/O 失败时仍须报告真实结果。具体协调记录、故障恢复和文档局部撤销后的冲突处理由实现设计落实，不在此虚构全局事务原子性。

### 4. 文件与虚拟文档

- 补齐工作区 stat、目录枚举、创建目录、写入、复制、改名、删除、原子替换与元数据查询，以及文件变化观察和分页发现。保持忽略规则、取消和溢出重扫约定可辨识。
- 工作区外访问必须经过用户选择文件或目录，授权限定目标、必要操作、实例及生命周期，不扩展为整个磁盘访问权。继续校验规范化路径、符号链接边界、目录穿越、设备路径和备用数据流。
- 首发支持只读虚拟文档及与当前文件的差异比较。历史或生成内容不要求先落临时文件；Tab 仍表示一份文件资源内容，不成为任意工具页面。
- 完整可写文件系统提供者和虚拟目录管理不在首发范围。网络资源身份不自动意味着已支持远程工作区。

### 5. 命令、贡献点与宿主交互

- 完善命令注册、发现、参数校验、调用、返回值、错误和异步取消；命令调用不得成为绕过能力或权限检查的通道。
- 支持稳定的宿主菜单位置、分组排序、上下文条件、动态可见及启用状态。目标包含编辑器、文件树、Tab 和选区等既有原生交互位置。
- 提供快选、输入、文件和目录选择、保存选择、通知、确认、进度及取消的公开入口，明确异步结果、关闭、实例归属与资源释放。
- 保留左侧窗口显隐按钮组与右侧插件自身工具按钮组的职责。插件业务、按钮内容和图标由插件提供，不因新增宿主贡献点转回宿主专属逻辑。

### 6. 诊断与语言提供者

- 多个插件可以同时发布诊断，宿主统一显示并标明来源。诊断含 owner、严重程度、位置、关联信息、修复及 revision；每个实例只能更新或清除自己的集合，禁用后撤销。
- 修复进入统一编辑流程。直接诊断复用既有问题模型，不另建一套与 LSP 无关的问题系统。
- 直接语言提供者与 LSP 复用呈现、取消、版本校验和编辑路径。首发优先完善引用查找、代码修复和函数参数提示；保留既有补全、格式化、Rename 和结构能力。
- 新增直接提供者先标记实验，独立插件验证后再决定稳定。允许多个诊断源不代表自动允许多个 LSP 同时运行；保留有效用户选择，不按安装顺序覆盖提供者。
- 明确实际 LSP feature matrix；服务器支持某方法不能代替宿主具备对应消费者。

### 7. 通用树、装饰与原生行为

- 树接口支持展开时加载、分批加载、刷新、选择、节点菜单、取消和错误呈现。插件提供领域节点，宿主管理虚拟化、滚动、焦点和键盘导航。
- 首发装饰覆盖行号旁图标、文字范围标记、悬浮说明及编辑后位置跟踪，明确关闭文档和禁用实例后的清理。复杂行间交互和任意嵌入式面板后续补充。
- 原生 UI 继续使用既定行为基座及本地外观层，不引入 WebView。新增界面覆盖中英文、深浅主题、焦点、键盘、中文 IME、滚动、缩放及基础无障碍语义。

### 8. 网络、密钥与插件协作

- 通用 HTTP 包含方法、头、请求体、流式响应、取消、代理、TLS、重定向及目标权限检查。流和大对象使用有界分块与回压，不强迫单个 JSON 承载所有内容。
- 网络传输与密钥访问分别协商和授权；允许读取密钥不代表允许发送到任意地址。现有图片网络和依赖下载用途不充当通用 HTTP。
- 首发支持用户提供 Token/API Key，宿主通过系统安全设施提供按插件隔离的读取、更新、删除及撤销。密钥不写普通设置、日志或普通更新快照，不借此接管外部 Git 的凭据管理。
- 浏览器登录、OAuth 回调和账号会话不作为首发验收条件，仅保留实验设计方向，不把未实现的空接口宣称可用。
- 禁止直接读取其他插件的私有文件或密钥。跨插件协作走公开服务契约，并校验调用方权限，不能以提供者权限代替调用方授权。

### 9. 状态、配置与生命周期

- 完善私有目录的查询、枚举、改名和删除、缓存及临时资源生命周期；延续现有配额、快照、迁移和失败恢复机制。
- 完善配置类型、校验、变更事件和适用的热应用方式，保留用户与项目设置边界。敏感值指向密钥能力，不成为普通设置字段。
- 提供可释放 timer 与 debounce 基础能力。定时器、订阅、网络请求、进程和贡献属于实例，禁用或退役时统一清理。
- 默认按命令、语言、视图或服务需求激活。声明式资源包无需空 WASM；按需激活不能使声明的命令或视图入口在启动前不可发现。
- 持续工作须声明后台用途，仍遵守信任、权限、资源预算和清理规则，不扩大为任意任务编排系统。

### 10. 稳定性、SDK 与交付

- 稳定接口与实验接口显式区分；包版本、协议版本与能力版本分别管理。延续 required/optional 协商和明确降级，实验标识不能替代权限检查或基本质量验收。
- 为首次公开后的稳定契约明确兼容演进、弃用、最低宿主和迁移规则；具体版本窗口与方法签名在实施中补齐，不假称已在本轮确定。不重新引入已拒绝的历史协议运行分支。
- SDK 同步提供类型、错误、取消、句柄释放、事件循环及独立范例，完善服务数据类型和有界事件流。公开能力不得仅有协议类型而无宿主消费或可用 SDK 路径。
- 复用现有独立构建、隔离开发实例、重载和日志机制，补充新接口覆盖。读者文档随实现更新到站点，维护者方案不复制为第二份 SDK 正文。
- Windows 优先验收；其他平台保持既有兼容目标并明确不可用原因，不将未执行的跨平台测试标为通过。

## Testing Decisions

### 主测试接缝（已确认）

统一采用“独立插件包 → 正式 SDK/包准入 → 公开 Manager → 宿主文档、文件、语言和原生 UI 消费者 → 用户可观察结果”的现有端到端接缝。扩展正式能力契约，不增加专属测试宿主 API，不把内部辅助函数的调用次数当作产品验收。

运行时集成测试验证协议、权限、实例隔离与清理；GPUI 集成和 Windows 原生交互验证实际输入、焦点、呈现、文档修改及撤销。它们是同一产品入口的不同验证层，不另造与真实插件不同的执行路径。纯校验逻辑可在现有模块就近补充测试，但不能替代真实包验收。

### 可复用先例

- 既有 `editor_edit` 测试以独立命名的真实 SDK 包经过 Package/Manager 检验文档请求及公开输出，适合作为编辑契约基础。
- `pure_language_completions`、`language_editing` 和 `xml_structure` 已覆盖真实语言提供者、结果校验和退役，可复用为直接语言结果与过期拒绝先例。
- `scoped_instances`、`plugin_services`、`hot_update` 等验证私有数据、调用方权限、提供者失效和更新恢复，可承接新增句柄和密钥生命周期验证。
- `development_projects` 已通过真实目录候选和公开管理器验证开发重载及失败恢复，复用其隔离开发路径。
- 既有 GPUI 文档、原生 UI、宿主消息和语言交互测试继续承接用户可见行为。历史通过记录只作为先例，不算本规格新增场景的通过证据。

### 行为验收矩阵

| 场景 | 必须观察到的结果 |
| --- | --- |
| 项目脚手架＋文档批量转换 | 同一文件/编辑接口完成目录选择、创建、修改、冲突预览、取消及结果报告 |
| 多文件 Rename＋批量格式转换 | 已打开脏文档不被磁盘写入覆盖；未打开文件按确认结果保存；过期版本拒绝 |
| 批量修改与整组撤销 | 文本修改、创建、改名和删除可在会话内恢复；任一目标出现后续变化时整组自动撤销停止；普通 Undo 不改其他文件 |
| 提交/撤销故障 | 注入 I/O 失败、取消、文件消失及目标关闭，呈现真实完成部分和恢复状态，不伪称原子成功 |
| 拼写检查＋配置检查 | 不启动 LSP 也能发布诊断、定位并修复；来源可辨，各自不能清空对方集合，禁用即清理 |
| Todo 树＋数据库对象树 | 同一树接口按需/分批加载、刷新、选中、菜单、取消、错误重试及键盘导航 |
| 书签＋覆盖率 | 同一装饰接口呈现图标、范围及说明；编辑后位置一致，主题变化和关闭清理正确 |
| Git 历史内容＋生成文本 | 无临时文件的只读虚拟文档可导航、刷新及比较，不允许未经声明的写入 |
| 两种独立语言提供者 | 引用、修复、参数提示通过正式消费路径，取消/过期结果拒绝；既有补全、格式化、Rename、结构不回退 |
| 云翻译＋代码托管查询 | 通用 HTTP 与安全 Token 存储可复用；流式读取、代理、取消、错误、重定向权限及资源释放可观察 |
| 双插件权限隔离 | 无法读取对方密钥/私有文件；服务调用不借权；日志和普通快照不含密钥 |
| 外部文件授权 | 用户选择后只能访问限定目标及操作；越界、符号链接逃逸和失效句柄拒绝 |
| 声明式包＋按需工具包 | 无空 WASM 要求，入口可发现，需要时激活；并发首次调用不导致重复实例，禁用清理后台资源 |
| 事件与资源压力 | 有界队列产生明确溢出或补读路径，大数据不无限累积；反复加载/卸载不持续泄漏 |
| 新旧 SDK 与隔离开发实例 | 能力缺失可诊断、optional 可降级；正常用户数据不受测试影响，失败重载保留旧版 |
| 原生交互与无障碍 | 中英文、深浅主题、IME、键盘、焦点、滚动、缩放及基础语义符合本地控件契约 |

上述消费者是验证接口通用性的独立夹具或范例，不等于首发必须交付完整数据库产品、翻译产品或 Git 扩展。受控外部服务可用于确定性失败与流测试，但测试必须经过正式插件网络接口。

### 执行要求

- 实施阶段先运行受影响模块的行为测试，再执行 `cargo fmt --check`、`cargo test --workspace --exclude editor-app` 和 `cargo check --workspace`。修改应用时追加相关应用测试和 Windows 原生验收。
- 实际 WASM 包测试先用当前 Cargo、宿主 `--plugin-cargo`/正式构建入口准备夹具，再显式运行相应 ignored 测试；不执行历史脚本或归档代理脚本，不静默安装缺失工具链。
- 每个阶段按既定仓库要求完成检查，不因减少工单数量省略验证，也不把 ignored 跳过或编译通过当作功能验收。
- 当前只是规格文档；本轮仅验证文档结构、链接和差异，不执行无关 Rust 构建，不宣称上表任何新能力已通过。

## Out of Scope

- AI 插件功能、AI 多文件修改专用流程、行内 AI 补全、自主代理、聊天参与者、模型市场及完整 AI 工作台。通用文本、编辑与网络能力仍为其他插件建设。
- 首发完整浏览器登录、OAuth/账号会话的稳定交付；只保留后续实验设计方向。
- 完整可写虚拟文件系统、虚拟目录管理、复杂自定义二进制编辑器及其完整编辑生命周期。
- 测试控制器、SCM Provider、完整高级调试、通用任务图及终端 shell 领域协议扩展。现有运行、调试与终端功能继续保留，相关专题独立推进。
- 任意复杂行间交互控件、完整多语言本地化 SDK、通用拖放/粘贴家族及完整任意状态栏/资源徽标能力；保持未来扩展空间，不以原审计 P1 自动纳入本次交付。
- 多 LSP 同时运行和任意结果仲裁、完整语义索引、全部高级语言特性一并实现。
- Notebook、远程/容器、多根/多窗口、协作编辑、WebView UI、在线插件市场或私有 GitHub 分发。
- 跨重启整组 Undo、任意外部副作用回滚、所有磁盘故障下全局原子提交。
- 强制推送、重写历史、关闭父设计议题或发布软件版本。2026-10-09 用户已授权实施全部七单、按现有约定提交推送并关闭已验收的工单；授权包括自动创建执行会话与子代理。

## Further Notes

### 依据与状态

- [完整性审计](plugin-api-ecosystem-audit/README.md)提供 58 项检查及外部来源；它是历史调查，不是缺少 58 个接口，也不自动授权实施全部 P1/P2。
- [平台规格](plugin-api-platform.md)、[UI 解耦规格](plugin-ui-decoupling.md)、[XML 语言规格](xml-language-tools.md)、[插件打包与隔离开发规格](plugin-development-packaging.md)为复用基础。
- [领域约定](../../agents/domain.md)、[议题约定](../../agents/issue-tracker.md)和[分诊词汇](../../agents/triage-labels.md)继续适用。
- [既有平台验收](../verification/plugin-api-contract-verification.md)及[开发运行验收](../verification/plugin-development-packaging.md)仅作为先例，不继承完成状态。
- 编写时 HEAD 为 `48e33ba`。工作区另有终端方案及插件索引改动，不属于本专题成果；本方案不修改该专题的决策或进度。实际实施前重新核对最新状态。
- 当前 Git origin 指向 T-miracle/Nanobug；历史议题约定中的 T-miracle/Editor 是旧仓库名称。发布前以当前仓库和稳定标识查重，并记录真实设计议题 URL。

### 本规格与历史基线的关系

本轮明确扩展普通插件的资源、文件写入、诊断、云查询及安全密钥接口，并引入只读虚拟资源；这些已确认决策优先于将插件限制为初期声明式语言包的历史条款。原生 Git 凭据处理、工作区信任、唯一文档状态、权限隔离、两阶段更新和数据保护约束继续有效。

用户在本轮依次确认基础社区覆盖、稳定核心与实验扩展、暂不实现 AI、多文件保存规则、Token 接入、外部文件选择授权、只读虚拟文档、多源诊断、基础装饰、数据隔离、通用树、直接语言提供者、条件激活和会话内整组撤销。测试接缝及七单拆分已于后续执行请求中确认，不重复询问。

### 后续组织建议

已按用户要求形成[七张工单草案与测试安排](../tickets/plugin-api-community-foundation/README.md)：文档与只读资源、命令与交互分别起步，随后交付文件事务、诊断与语言、树与装饰、云服务，最后贯通按需激活、配置热应用与兼容升级。SDK、兼容设计、站点文档和独立插件验收贯穿各单。粒度、九条直接阻塞边与测试安排已确认；用户已授权连续执行，实际进度见工单目录。

方法签名、枚举、限额、版本窗口和具体撤销协调算法仍需在实施中依据本规格落实。若发现无法同时满足已确认行为与唯一文档状态约束，应报告具体矛盾，不擅自弱化冲突保护或另造文本状态。
