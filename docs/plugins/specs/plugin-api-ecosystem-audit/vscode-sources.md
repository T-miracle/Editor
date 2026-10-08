# VS Code 通用扩展 API 一手来源调查

日期：2026-10-08。状态：调研，非确认实施范围。

本笔记只建立比较基线，不据此判断 Nanobug 某项能力缺失，也不授权实现任务、Notebook、远程、多窗口或 AI 等新产品范围。Nanobug 的缺口必须进一步沿公开协议、SDK、Runtime、宿主消费方和真实插件验收链核对。

## 取证基线

调查时，微软[稳定更新端点](https://update.code.visualstudio.com/api/update/win32-x64/stable/latest)返回 VS Code **1.141.0**，commit `2a59476c9bfcb90b3ddc372c36762471b7dfad1c`。以下 API 名称直接核对该版本的[稳定类型声明](https://github.com/microsoft/vscode/blob/1.141.0/src/vscode-dts/vscode.d.ts)，未拿主分支的新 API 当成已发布能力。网页文档可能继续更新，涉及稳定性时以固定 tag 类型声明为准。

网页 API 总表多次抓取超时，因此通过 HTTPS 读取微软仓库上述固定 tag 的原始声明；没有下载、执行 VS Code 或扩展。只做事实调查，未运行构建或产品测试。

VS Code 通过声明式贡献点和动态 API 两类入口扩展。扩展可以注册命令、语言、配置和 UI 贡献，但没有宿主 DOM 或任意宿主 CSS 的访问权；这支持“公开机制、宿主绘制”的边界思想，不能据此要求 Nanobug 引入 WebView。[能力概览与限制](https://code.visualstudio.com/api/extension-capabilities/overview)

## 已稳定公开的能力分类

“基础”表示本调研建议优先审计的通用生态底座；“领域”表示应完整纳入路线图，但是否首发取决于产品是否承诺该领域。优先级属于建议，不是 VS Code 官方分类。

| 能力域 | 已核实的 VS Code 入口及语义 | 对 Nanobug 的审计建议 |
| --- | --- | --- |
| 文档身份与快照 | `Uri`、`TextDocument`、`version`、`getText`、`positionAt`、`offsetAt`；位置以 UTF-16 code unit 表示。[稳定声明](https://github.com/microsoft/vscode/blob/1.141.0/src/vscode-dts/vscode.d.ts#L49) | 基础：明确 URI、未保存文档、版本、位置编码，避免插件自行猜测 Unicode 偏移。 |
| 编辑器上下文 | `activeTextEditor`、`visibleTextEditors`、selection/visible-range/editor-change 事件；`showTextDocument`、`revealRange`、`insertSnippet`。[稳定声明](https://github.com/microsoft/vscode/blob/1.141.0/src/vscode-dts/vscode.d.ts) | 基础：查询、订阅、定位和修改分开；不能只暴露“当前文件路径”。 |
| 编辑与撤销 | `TextEditor.edit` 可控制前后 undo stop；`WorkspaceEdit` / `workspace.applyEdit` 承载跨文档文本及文件创建、重命名、删除。纯文本和包含资源操作的失败语义不同，不能宣称任意跨文件操作完全原子回滚。[稳定声明](https://github.com/microsoft/vscode/blob/1.141.0/src/vscode-dts/vscode.d.ts) | 基础：revision 校验、重叠编辑、预览/确认、undo 分组、部分失败及恢复必须成为契约。 |
| 文档与文件生命周期 | 打开、改变、关闭、保存事件；`onWillSaveTextDocument`；文件 create/delete/rename 的 will/did 事件。[稳定声明](https://github.com/microsoft/vscode/blob/1.141.0/src/vscode-dts/vscode.d.ts) | 基础：保存前处理、导入路径重写等必须用可取消/有预算的生命周期机制。 |
| 命令与上下文 | 注册/调用命令，清单贡献快捷键与菜单；`when` 条件控制可见性。[常用能力](https://code.visualstudio.com/api/extension-capabilities/common-capabilities) | 基础：类型化参数、返回值、可用条件、执行上下文；菜单位置应公开而非硬编码插件 ID。 |
| 配置、状态和私有存储 | `getConfiguration` / `onDidChangeConfiguration`；`workspaceState`、`globalState`、`storageUri`、`globalStorageUri`；可选同步键。[常用能力](https://code.visualstudio.com/api/extension-capabilities/common-capabilities) | 基础：作用域、默认值、来源、schema、迁移、删除/禁用保留策略；秘密不能混入普通状态。 |
| 标准交互 | 通知、QuickPick、InputBox、文件打开/保存选择器、`withProgress`；支持取消的操作应传播 token。[常用能力](https://code.visualstudio.com/api/extension-capabilities/common-capabilities) | 基础：插件不应为每次选择、确认、输入、后台进度自己造面板。 |
| 树与工作台贡献 | `TreeDataProvider` / `TreeView`，含懒加载、刷新、选择、展开与拖放；容器、菜单和视图由贡献点接入。[Tree View](https://code.visualstudio.com/api/extension-guides/tree-view) | 基础：数据源接口和宿主原生树行为分离；文件树、插件窗口栏、工具按钮组明确各自插槽。 |
| 编辑内显示与文件装饰 | `createTextEditorDecorationType`、`setDecorations`、`registerFileDecorationProvider`；状态栏项、输出/日志通道。[稳定声明](https://github.com/microsoft/vscode/blob/1.141.0/src/vscode-dts/vscode.d.ts) | 基础：范围装饰、行号边标、徽标、hover 内容、主题适配、回收和大文件性能。 |
| 主题与资源 | 主题、文件图标、产品图标及声明式语法资源为扩展能力。[能力概览](https://code.visualstudio.com/api/extension-capabilities/overview) | 基础：图标、颜色 token、字体/比例、资源 URI、亮暗主题应能由插件声明并由宿主安全绘制。 |
| 自定义文件编辑器 | `CustomTextEditorProvider` 复用文本模型；`CustomReadonlyEditorProvider` / `CustomEditorProvider` 面向其他数据模型；非文本自定义文档有保存、另存、恢复、备份和编辑事件。[Custom Editor](https://code.visualstudio.com/api/extension-guides/custom-editors) | 基础（已有 Image 等场景）：只会绘制预览不等于完整文件 Provider；重点检查 dirty/save/undo/backup、默认选择和恢复。 |
| 虚拟只读文档 | `TextDocumentContentProvider` 注册 URI scheme，并通知内容变化。[稳定声明](https://github.com/microsoft/vscode/blob/1.141.0/src/vscode-dts/vscode.d.ts) | 基础：diff 版本、生成内容、反编译结果不必落地临时文件。 |
| 文件系统提供者 | `FileSystemProvider` / `workspace.fs` 支持 scheme 对应的读写、目录、stat、watch 等；虚拟资源不保证存在本机路径。[Virtual Workspaces](https://code.visualstudio.com/api/extension-guides/virtual-workspaces) | 资源模型是基础；完整远程产品可后置。提前使用 URI/资源句柄，避免把所有接口锁死为本地路径。 |
| Workspace 与发现 | `workspaceFolders`、工作区文件夹变化、`RelativePattern`、`findFiles`、文件 watcher、配置作用域。[稳定声明](https://github.com/microsoft/vscode/blob/1.141.0/src/vscode-dts/vscode.d.ts) | 基础：排除规则、只读资源、文件变更、无工作区状态；多根可后置但身份不要用进程全局单一路径。 |
| 语言声明与基础编辑 | grammar、snippets、语言匹配、括号/注释/缩进等声明式语言配置。[能力概览](https://code.visualstudio.com/api/extension-capabilities/overview) | 基础：Nanobug 可继续 Tree-sitter，无需模仿 TextMate；应核对注入语言、注释、缩进和片段等实际行为。 |
| 语言功能 Provider | diagnostics、completion、inline completion、hover、signature、definition/declaration/type definition/implementation、references、symbols、rename、formatting、code action、CodeLens、document links/colors、folding、selection range；既能直接注册，也能通过语言客户端桥接 LSP。[Programmatic Language Features](https://code.visualstudio.com/api/language-extensions/programmatic-language-features) | 基础：有 LSP 进程入口不等于这些能力均能显示、交互或由 WASM 插件直接提供。 |
| 进一步语言与编辑 Provider | semantic tokens、inlay hints、call/type hierarchy、linked editing、paste/drop edits 在稳定声明中有对应注册函数。[稳定声明](https://github.com/microsoft/vscode/blob/1.141.0/src/vscode-dts/vscode.d.ts#L15063) | 建议完整列入语言审计：优先解决 Provider 聚合/冲突、懒解析、取消、过期结果和统一编辑提交机制。 |
| 终端 | `createTerminal`、`Pseudoterminal`、终端链接/配置 Provider、环境变量集合；shell integration 提供执行命令、开始/结束事件、读取该次命令输出流。[稳定声明](https://github.com/microsoft/vscode/blob/1.141.0/src/vscode-dts/vscode.d.ts) | 基础（已有终端）：PTY、尺寸、进程退出只是底层，还应核对结构化命令执行、cwd、exit code 与输出订阅。 |
| 任务 | `TaskProvider`、任务定义、`ProcessExecution` / `ShellExecution` / `CustomExecution`；任务生命周期、problem matcher 接入统一任务体验。[Task Provider](https://code.visualstudio.com/api/extension-guides/task-provider) | 领域：支持构建/运行时应有发现、配置、执行、取消和结果，不能仅有“启动进程”。 |
| 调试 | debug configuration Provider、adapter descriptor/tracker、DAP、会话事件及断点 API。[Debugger Extension](https://code.visualstudio.com/api/extension-guides/debugger-extension) | 领域：若已有调试承诺，核对启动/附加、暂停栈帧、变量、断点、控制台和取消/退出均为通用接口。 |
| 测试与覆盖率 | `TestController` 发现树；运行/调试/覆盖率 profile；状态、消息、输出和按需解析。[Testing API](https://code.visualstudio.com/api/extension-guides/testing) | 领域：社区测试插件需要宿主统一结果模型；把输出印到终端不能替代测试 API。 |
| SCM 与评审 | `scm.createSourceControl` 注册任意版本控制资源组；资源状态、装饰和命令由 Provider 提供。`comments.createCommentController` 为代码评审提供线程/评论入口。[SCM](https://code.visualstudio.com/api/extension-guides/scm-provider)、[稳定声明](https://github.com/microsoft/vscode/blob/1.141.0/src/vscode-dts/vscode.d.ts) | 领域：宿主内置 Git 命令不等于公开 SCM；应允许其他 VCS、远程评审及差异数据源复用 UI。 |
| 凭据与认证 | `ExtensionContext.secrets`；`authentication.getSession`、注册认证 Provider、session 变化事件。[稳定声明](https://github.com/microsoft/vscode/blob/1.141.0/src/vscode-dts/vscode.d.ts#L18096) | 基础：线上生态需要 secret store、账户/作用域、撤销，避免插件明文保存令牌；认证 UI 与秘密访问分别授权。 |
| 系统集成 | clipboard、`env.openExternal`、`env.asExternalUri`、URI handler、locale、本地化和日志/遥测偏好接口。[稳定声明](https://github.com/microsoft/vscode/blob/1.141.0/src/vscode-dts/vscode.d.ts) | 基础：剪贴板、外链、登录回调、日志及隐私偏好常被忽略；不能透传任意 Shell 字符串替代。 |
| 网络与原生执行边界 | 桌面扩展宿主运行 Node.js，Web 扩展运行 WebWorker，环境能力不同；Web 侧网络受浏览器和 CORS 约束。[Extension Host](https://code.visualstudio.com/api/advanced-topics/extension-host)、[Web Extensions](https://code.visualstudio.com/api/extension-guides/web-extensions) | 对 WASM 特别重要：需要明确 HTTP/流式响应/取消/超时/代理/证书、进程/环境/stdio 的受控能力。不能误以为 VS Code 没有专用 `vscode.http` 就不需要网络能力。 |
| Notebook | serializer、controller/kernel、cell execution、输出 MIME renderer、消息通道、文档/单元格事件。[Notebook](https://code.visualstudio.com/api/extension-guides/notebook) | 领域：纳入路线图，已有通用画布不等于单元格执行/输出持久化语义。 |
| AI / Chat / Tools / MCP | 稳定 1.141.0 包含 `chat.createChatParticipant`、`lm.selectChatModels`、`lm.registerTool`、`lm.registerMcpServerDefinitionProvider`、`lm.registerLanguageModelChatProvider`。[稳定声明](https://github.com/microsoft/vscode/blob/1.141.0/src/vscode-dts/vscode.d.ts#L20116)、[LM](https://code.visualstudio.com/api/extension-guides/ai/language-model)、[Tools](https://code.visualstudio.com/api/extension-guides/ai/tools) | 领域：不是所有编辑器首发必需；优先打通通用网络、流、编辑预览、进度、取消、认证，后续再承接 chat/tool 产品模型。 |
| 生命周期与互操作 | `Disposable`、`CancellationToken`、`ExtensionContext.subscriptions`、activation events、扩展依赖和 `extensions.getExtension(...).exports`。[稳定声明](https://github.com/microsoft/vscode/blob/1.141.0/src/vscode-dts/vscode.d.ts)、[清单](https://code.visualstudio.com/api/references/extension-manifest) | 基础：每个注册/订阅/请求应有作用域、撤销、超时和故障路径；跨插件服务要有版本，不应公开内部对象。 |
| 信任与执行位置 | manifest 声明 untrusted/virtual workspace 支持；`workspace.isTrusted`、授权事件；`extensionKind` 区分靠近 UI 或 workspace。[Workspace Trust](https://code.visualstudio.com/api/extension-guides/workspace-trust)、[Extension Host](https://code.visualstudio.com/api/advanced-topics/extension-host) | 基础：信任变化能撤销能力；远程可后置，但进程、文件、URI 所属执行位置需要清楚。 |
| SDK、兼容与交付 | `engines.vscode` 标明最低兼容版本；vsce 打包 VSIX/平台目标；Extension Development Host 可按指定版本运行集成测试。[清单](https://code.visualstudio.com/api/references/extension-manifest)、[发布](https://code.visualstudio.com/api/working-with-extensions/publishing-extension)、[测试](https://code.visualstudio.com/api/working-with-extensions/testing-extension) | 基础：除了 API，还需独立样例、契约测试、模拟/真实宿主测试、最低宿主声明、弃用政策、包校验和迁移说明。 |

## 必须区别的“宿主有”与“稳定公开”

VS Code 的 proposed API 可能已有实现，但不承诺稳定，也不应作为普通 Marketplace 扩展的稳定依赖；实验入口与稳定入口明确隔离。[Using Proposed API](https://code.visualstudio.com/api/advanced-topics/using-proposed-api)

以下逐项核对 **1.141.0** 的稳定声明和同 tag proposals，不能把编辑器界面的存在当成稳定公开证据：

| 能力 | 该基线的结论与证据 |
| --- | --- |
| 文件名搜索 | `workspace.findFiles` 稳定。[稳定声明](https://github.com/microsoft/vscode/blob/1.141.0/src/vscode-dts/vscode.d.ts) |
| 编程调用全文搜索 | `findTextInFiles` / `findTextInFiles2` 仍在 proposed 文件；不能称“VS Code 稳定通用全文搜索 API”。[旧接口](https://github.com/microsoft/vscode/blob/1.141.0/src/vscode-dts/vscode.proposed.findTextInFiles.d.ts)、[新接口](https://github.com/microsoft/vscode/blob/1.141.0/src/vscode-dts/vscode.proposed.findTextInFiles2.d.ts) |
| 自定义搜索后端 | 文件/文本 SearchProvider 及其第二版仍属 proposed。[FileSearchProvider2](https://github.com/microsoft/vscode/blob/1.141.0/src/vscode-dts/vscode.proposed.fileSearchProvider2.d.ts)、[TextSearchProvider2](https://github.com/microsoft/vscode/blob/1.141.0/src/vscode-dts/vscode.proposed.textSearchProvider2.d.ts) |
| 时间线 Provider | `registerTimelineProvider` 在 proposed；不能以 Git 的时间线 UI 推导所有插件都有稳定入口。[Timeline](https://github.com/microsoft/vscode/blob/1.141.0/src/vscode-dts/vscode.proposed.timeline.d.ts) |
| 终端补全 Provider | `registerTerminalCompletionProvider` 在 proposed。[Terminal Completion](https://github.com/microsoft/vscode/blob/1.141.0/src/vscode-dts/vscode.proposed.terminalCompletionProvider.d.ts) |
| 任意终端数据流 | `terminalDataWriteEvent` 在 proposed，区别于稳定 shell integration 的“某次命令输出流”，以及插件自己提供的 `Pseudoterminal` 流。[proposed 目录](https://github.com/microsoft/vscode/tree/1.141.0/src/vscode-dts)、[稳定声明](https://github.com/microsoft/vscode/blob/1.141.0/src/vscode-dts/vscode.d.ts) |
| 宿主 UI 内部 | 不能直接操作宿主 DOM/内部样式；公开 WebView 内容与宿主 UI 不同。[限制](https://code.visualstudio.com/api/extension-capabilities/overview#restrictions) |
| Git 内部实现 | 通用 SCM API 不等于任意内置 Git 方法都在稳定 `vscode` 命名空间中；消费内置 Git 扩展 exports 需要单独契约核对。[SCM](https://code.visualstudio.com/api/extension-guides/scm-provider)、[稳定声明](https://github.com/microsoft/vscode/blob/1.141.0/src/vscode-dts/vscode.d.ts#L17463) |

## 面向 Nanobug 的综合建议（设计判断）

1. **先保证社区能组合基础能力。** 建议首发审计：文档快照与安全编辑、URI/文件变化、命令/配置/生命周期、标准交互、视图/树/装饰、语言 Provider、网络/进程、secret/认证、日志/取消、独立 SDK 与兼容测试。每项都检查“声明 → 注册 → 发现/查询 → 调用 → 事件 → 撤销/失败”，不能只数类型名称。
2. **基于现有产品能力补公开闭环。** 如果宿主已经支持搜索、Git、运行调试或历史，却只有内部函数，社区依旧无法复用。该缺口的优先级可高于竞争产品尚未稳定的对应 API；例如全文搜索没有必要等待 VS Code 稳定后才设计。
3. **提前决定必须稳定的身份与语义。** 文档/资源/工作区/实例 ID、位置编码、版本、错误分类、取消、流背压、保存/undo/备份、provider 冲突选择是后续所有领域 API 的基础。先固定这些可减少已发布插件的破坏性迁移。
4. **领域 API 做路线图而非全部首发。** Notebook、AI Chat、测试覆盖率、多根/远程、协作/评审应说明“已支持／计划／明确不支持”。社区开发者最需要明确能力边界、可查询的版本和扩展申请路径，而不是看起来万能却缺少实现语义的接口。
5. **保持原生 UI 路线。** 对照 VS Code 的用户能力和生命周期，而非复制 HTML/DOM 机制。Nanobug 的布局、窗口按钮组、工具按钮组、Image 预览等应通过通用原生资源与视图接口承载。

限制：这不是 VS Code 全部成员的逐一审计，也不是 Nanobug 实现验收；固定版本之外新增的 proposed API、官方内置扩展的私有能力和所有第三方插件自建接口不作为稳定公共基线。
