# 内置终端与运行、调试统一面板方案

日期：2026-10-09

稳定标识：`builtin-terminal-run-debug`

状态：2026-10-09 四张工单已按用户授权逐步实施与验收；终端内置、统一运行/调试面板及旧用户迁移的实际结果见 [最终验收](../verification/builtin-terminal-04-2026-10-09.md)。Windows 原生操作及系统输入法已验证，其他平台与历史测试夹具限制另有记录；未发布软件版本，父规格议题保持不变。

目标跟踪器：T-miracle/Nanobug GitHub Issues。发布完整规格及[四张实施工单与测试安排](../tickets/builtin-terminal-run-debug/README.md)，应用 `ready-for-agent`；按直接依赖顺序实施、验收与交付。

## Problem Statement

用户希望终端成为 Nanobug 安装后即可使用的内置功能，不再单独安装、启用和维护终端插件。目前终端的界面、Shell、终端模拟及历史恢复由独立 WASM 包提供，宿主运行功能又依赖该包提供的执行和 Shell 配置能力。只移动源代码目录不能解除这个运行与发行依赖。

普通运行已经能使用终端 Tab，但构建和准备输出、调试程序输出以及调试检查区分散在多个底部区域。同一个任务在准备、运行和调试时切换展示位置，占用重复空间，也使关闭、停止和再次运行的行为难以预测。当前调试输出仅为文本，无法直接向需要交互的被调试程序输入内容。

用户同时要求恢复上游 Alacritty 终端核心，并保留已形成的终端交互、配置、主题和历史数据，避免迁移后出现两个终端入口或丢失已有运行配置。

## Solution

将终端作为宿主内独立的原生功能模块交付，直接使用上游 `alacritty_terminal` 依赖和本地 GPUI 控件。编辑器无需安装终端插件即可打开 Shell，并为程序运行和调试提供终端输入输出。

普通 Shell、构建、运行和调试统一使用一个底部终端面板，各自通过 Tab 区分。同一次执行的准备步骤和目标程序输出归入同一个任务 Tab；选择调试 Tab 时，在面板内部显示调试控制、变量、调用栈和断点，不另外增加输出或检查面板。

运行或调试中的任务 Tab 关闭前提示确认，确认后停止对应任务并关闭，取消则保持原状。隐藏整个面板不停止任务。同一配置再次执行时复用尚未关闭的对应任务 Tab，并清空上次输出；原 Tab 已关闭则新建。普通“运行”不重复启动仍在运行的同一配置，显式“重新运行”才停止并重启。

语言配置、构建规则和具体调试器继续由插件提供。它们通过公开、受控且可复用的能力与内置终端协作，宿主不引入针对 Rust、CodeLLDB 或特定插件身份的日常业务分支。已有终端数据和配置引用有限迁移，保留旧数据备份。

## User Stories

1. As an editor user, I want a built-in terminal available after installation, so that I do not need to install a terminal plugin before using a shell.
2. As an editor user, I want the terminal to use the upstream Alacritty core, so that the agreed terminal engine is restored without a WASM-specific fork.
3. As an editor user, I want one bottom terminal panel for shells, builds, runs and debugging, so that output does not create competing panel areas.
4. As an editor user, I want separate tabs for independent sessions, so that their output and input remain isolated.
5. As an editor user, I want task launches to leave my ordinary shell tabs intact, so that running a project does not replace my interactive work.
6. As an editor user, I want preparation and program output grouped in one task tab with step identification, so that I can trace the whole execution.
7. As an editor user, I want standalone builds and plugin development tasks to use the same panel, so that they do not create additional output areas.
8. As an editor user, I want failed or cancelled preparation to prevent later steps and program launch, so that unfinished preparation cannot start an invalid task.
9. As an editor user, I want selecting a task to reveal its exact terminal tab, so that controls and output refer to the same session.
10. As an editor user, I want different configurations to run concurrently, so that independent tasks do not block each other.
11. As an editor user, I want repeated Run on an active configuration to locate its existing session, so that accidental duplicate programs are not started.
12. As an editor user, I want an explicit rerun action to wait for the old process to stop, so that replacement instances do not overlap.
13. As an editor user, I want a subsequent execution to reuse its still-open task tab, so that completed runs do not accumulate unnecessary tabs.
14. As an editor user, I want a new tab when the previous task tab has been closed, so that a closed tab is not silently restored as the same view.
15. As an editor user, I want reused task tabs to show only the new execution output, so that old results are not mistaken for current results.
16. As an editor user, I want confirmation before closing an active run or debug tab, so that stopping a program is intentional.
17. As an editor user, I want cancelling that confirmation to preserve the tab and its program, so that cancelling has no destructive side effects.
18. As an editor user, I want closing a finished task tab to complete immediately, so that no irrelevant process warning is shown.
19. As an editor user, I want hiding the terminal panel to leave tasks running, so that I can recover editor space without ending work.
20. As an editor user, I want the last closed tab to hide the panel and reopening an empty panel to create a shell tab, so that existing terminal opening behavior remains consistent.
21. As an editor user, I want pause, continue and stepping controls inside the selected debug tab's panel, so that debugging stays in one place.
22. As an editor user, I want variables, call stacks and breakpoints available inside that panel, so that merging views does not remove inspection features.
23. As an editor user, I want to enter text into a program while debugging it, so that interactive command-line programs can be debugged normally.
24. As an editor user, I want debugger protocol messages isolated from terminal input, so that text intended for my program cannot become a debugger command.
25. As an editor user, I want the debug target launched exactly once, so that terminal integration does not create a second undebugged process.
26. As an editor user, I want late output and inspection responses confined to their original session, so that switching or reusing tabs cannot mix results.
27. As an editor user, I want existing terminal tab names, ordering, side placement and width behavior preserved, so that the migration keeps my familiar layout.
28. As an editor user, I want copy, paste, selection, context menus, scrolling and Chinese IME preserved, so that normal terminal interaction remains usable.
29. As an editor user, I want my terminal theme overrides and existing cursor behavior preserved, so that the built-in terminal fits my editor theme.
30. As an editor user, I want resizing and history restoration to avoid duplicate prompts and injected blank lines, so that the displayed session stays readable.
31. As an editor user, I want terminal settings, saved tabs, directories and historical output migrated with a backup, so that adopting the built-in terminal does not discard my data.
32. As an editor user, I want migration to be safe to repeat and isolated by workspace, so that retries do not duplicate sessions or overwrite unrelated data.
33. As an editor user, I want existing Shell run configurations to remain usable after migration, so that they no longer depend on the removed terminal plugin.
34. As an editor user, I want completed run and debug programs to remain stopped after restarting the editor, so that restoring their output does not execute old commands again.
35. As an editor user, I want only one terminal entry after migration, so that an old installed package does not duplicate the built-in feature.
36. As a plugin author, I want generic public execution and terminal capabilities to remain available, so that my plugin can use the built-in terminal without special identity handling.
37. As a language plugin author, I want to retain ownership of build rules and debugger integration, so that language-specific behavior stays independently maintained.
38. As an editor user, I want workspace trust and process permissions enforced after the migration, so that making the terminal built-in does not bypass execution restrictions.
39. As an editor user, I want failures to remain visible with a cleanup or retry path, so that a failed start, stop or migration does not leave hidden processes or lost output.
40. As an editor user, I want consistent light and dark themes, Chinese and English text, keyboard focus and resizing, so that the unified panel behaves like the rest of the native editor.

## Implementation Decisions

### 1. 原生终端模块与终端核心

- 终端的领域状态、终端模拟、Shell 配置、输入选择、历史恢复和面板集成迁入宿主内的独立功能模块，顶层应用仅组装和路由；不把业务代码堆入启动入口，也不仅移动目录后继续作为隐藏 WASM 插件加载。
- 采用上游 `alacritty_terminal` 依赖，移除终端对 `term-wm-vt100` 的运行依赖以及终端专用 WASM 适配、Guest 分发和包加载链。依赖版本在实施时核对适配性并锁定，不在本规格虚构版本或兼容性结论。
- 终端 UI 直接组合项目本地 GPUI 控件，遵循 gpui-base 行为基座及项目外观契约。终端本身无需继续经过插件声明式 UI 序列化链；其他插件的声明式 UI 能力保持可用。
- 复用已有 PTY 和受控进程实现，在现有模块边界内提取必要的通用能力。终端会话逻辑、进程资源和通用渲染职责分开，不为此次迁移复制另一套 ConPTY 实现或扩大全部运行时内部可见性。

### 2. 单一面板与会话关联

- 复用现有底部停靠和终端显隐入口。移除构建输出、调试输出和调试检查区的独立展示入口，内容统一进入终端面板；文档 Tab 栏仍只承载文件。
- 普通 Shell 使用独立 Tab；任务使用关联配置与执行身份的任务 Tab，不复用用户正在操作的普通 Shell。
- 同次执行的构建准备、启动前步骤、正式程序输出归入同一个任务 Tab，保留步骤标识、状态和错误。独立构建及现有插件开发、构建、打包任务也使用该面板。
- 选中调试 Tab 时，调试控制和检查内容在同一面板内部组合，显示断点、调用栈和变量。只调整展示归属，不删除现有断点导航、暂停、继续、步入、步过和步出能力。
- 配置身份、一次执行身份、终端 Tab 身份和调试暂停代次分别保留；复用 Tab 不复用已经结束的进程或过期句柄。不同配置可并行，输出、输入、控制和检查结果不能串入其他会话。
- 用户打开空面板时创建一个普通 Shell Tab；任务入口打开面板并创建目标任务 Tab 时不附赠无关 Shell。最后一个 Tab 关闭后隐藏面板，正常隐藏面板不结束任何任务。

### 3. 关闭、停止、复用与重新运行

| 操作或状态 | 确认后的行为 |
| --- | --- |
| 关闭正在准备、运行或调试的任务 Tab | 提示确认停止并关闭；取消无副作用；确认后按现有停止与进程树清理流程结束该任务，再关闭 Tab |
| 等待确认时任务自然结束 | 按实际状态关闭，不重复发出启动或停止请求 |
| 停止失败或未完成 | 保留可见任务状态及错误，不伪称成功关闭，也不留下不可见的活动任务 |
| 关闭已经结束的任务 Tab | 直接关闭，不再保留可重新显示的隐藏 Tab |
| 关闭普通 Shell Tab | 保留原有关闭 Shell 资源语义，本次不将任务关闭确认扩展为所有 Shell 的新交互 |
| 隐藏整个面板 | 仅隐藏；会话与进程保持原状态，可通过既有定位入口重新显示 |
| 对仍在运行的同一配置点击运行 | 定位已有任务，不再次执行准备步骤或启动副本 |
| 显式重新运行 | 先停止旧任务并确认退出，再执行正常校验、保存、准备和启动流程 |
| 再次执行且对应任务 Tab 仍存在 | 复用该 Tab；新一轮执行获准开始时清除上次输出，当前轮准备输出随后保留至程序结束 |
| 再次执行且原任务 Tab 已关闭 | 新建任务 Tab，不以旧隐藏状态复活已关闭 Tab |

- 任务 Tab 的关联不得仅凭可重名的显示名称推断。原 Tab 的旧事件不能写入复用后的新执行；用户关闭的 Tab 不能被迟到事件重新创建。
- 校验、权限或信任门禁拒绝启动时，不因一次未受理的运行点击清除已有任务输出。失败后保留可诊断的本轮输出。
- 独立停止、宽限后强制终止、立即停止以及关闭窗口或切换项目时的既有确认和清理规则保持有效；不增加脱离编辑器的后台运行模式。

### 4. 通用执行与调试契约

- 宿主运行系统和其他插件仍可通过通用契约执行、停止、查询、输入、定位和订阅会话。优先沿用 `interactive.execute` 与 `debug.session` 的公开语义；内置实现的注册不依赖已安装终端包，也不变成仅宿主可用的私有旁路。
- Shell 模板、可用解释器发现及其配置表单随终端内置化迁移；已有运行配置的来源和执行提供者引用一并迁移。其他语言的模板、校验、构建规则和工具发现仍由各插件承担。
- 调试器继续拥有调试目标的启动编排；通过通用、受控的终端创建和连接能力将目标接入 PTY。无论最终握手由哪方执行原生创建，都只有一次目标启动，运行系统不得额外启动一个普通程序副本。
- 调试程序 stdin/stdout 与调试协议传输使用独立通道。真实程序输入是本次新增验收内容，不能用普通文本输出控件或向调试协议写入文本代替。
- DAP、CodeLLDB 和语言专属映射留在调试插件，宿主只处理通用能力、资源归属和状态。公开扩展须允许其他合规插件接入；不增加具体调试器 ID 白名单或专属系统 API。
- 调试启动失败、握手超时、取消、插件退休及窗口退出均沿资源归属清理目标、PTY、适配器和相关订阅。启动中停止也必须有界结束，不能出现孤儿进程。
- 暂停检查继续校验会话和暂停代次，保留既有焦点与源码导航策略。适配器缺少所需能力时明确报告，不静默降级为无调试的普通运行。
- 涉及公开契约变更时，版本协商、权限、SDK、消费者、包版本与读者文档同步更新；具体字段和版本号在实施时沿现行契约细化，不预置未经核验的新接口。

### 5. 交互、主题与数据迁移

- 保留现有终端能力：Shell 命名与自定义名称、双击编辑并在 Enter 或失焦保存、Tab 拖动排序、左右侧栏与可拖动宽度、内边距、选中样式与图标、主题覆盖和默认竖线光标。
- 保留现有 Ctrl+C/V 复制粘贴映射、其他快捷键、有效文本选择、中文 IME、右键复制/粘贴/清空缓冲区以及滚动历史。复制菜单仍受有效选择限制；滚动条仅在可滚动时显示，并保留停止滚动后 1 秒隐藏的交互。任务停止使用明确任务控制，不借本次迁移擅改复制快捷键。
- 保留用户主题覆盖及与编辑器深浅主题的映射，不以恢复核心为由更改现有外观。新增核心支持的高级协议不自动扩展为全部 UI 功能的交付承诺。
- 将已安装终端的设置、Tab 名称和顺序、Shell 类型、工作目录、旧输出及面板布局迁入内置存储；按实际运行时解析的工作区数据范围读取，而非依据过时的扁平路径说明。
- 迁移先备份并验证，再提交新数据和完成标记；重复启动不重复导入，不覆盖迁移后的新状态。失败时保留原数据并给出可诊断状态，其他插件、其他工作区及既有 MeEditor 持久化标识不受影响。
- Alacritty 与旧终端核心的数据表示不同，采用有限的数据转换恢复可见内容、顺序和必要显示状态，不直接把旧核心内部格式当作新核心状态。历史恢复不注入“restored session; new shell”等提示或额外换行，不重新执行历史命令。
- 普通 Shell 的恢复只重建新的 Shell 进程；已结束或重启前的受管运行、调试任务恢复为非运行状态，不重放程序。历史输出容量继续受已有可配置限制约束。
- 连续改变终端宽高、恢复后再改变高度、交替屏幕和长行重排需要回归，不能用裁剪历史输出掩盖重复提示或额外空白问题。
- 成功迁移后撤销旧终端包的加载、贡献和执行注册，退役其独立构建与发行入口，防止重复入口。旧安装记录和配置来源的识别仅属于有限迁移，不成为通用运行系统的长期插件 ID 分支；用户数据备份不因停用旧包而删除。

### 6. 继续有效与被替代的规则

- 本规格替代插件平台与终端迁移旧方案中“终端必须作为独立 WASM 插件”的要求，以及终端专属独立包安装、更新和卸载交付要求。
- 本规格替代旧运行方案中“关闭任务输出 Tab 仅隐藏并继续运行”的规则，采用用户此次确认的停止确认与关闭语义；隐藏整个面板仍不停止程序。
- 本规格替代旧调试方案中目标仅走内部文本控制台的显示方式，新增受控 PTY 交互；单目标启动、插件拥有调试协议、进程树管理和暂停代次规则仍有效。
- 本规格替代 Shell 模板必须由终端插件提供以及构建、调试检查各自占据独立区域的约定，不修改其他语言插件的模板职责或编辑器文件 Tab 的定位。
- 工作区信任、插件权限和资源归属不因内置化放宽。内置 UI 与插件调用仍区分授权来源；受限工作区不得自动恢复 Shell 或启动任务。参数、目录、环境继续经结构化受控入口传递。
- 本规格未替代的运行配置编辑、执行前校验、保存门禁、顺序步骤、构建产物选择、主题与原生控件约束继续有效。

## Testing Decisions

### 主要测试接缝提案：编辑器原生操作入口

以现有编辑器应用集成入口作为唯一主要验收接缝：在隔离工作区和数据根中启动编辑器，安装实际语言/调试提供者但不安装终端插件，经实际终端、运行、构建、调试、关闭和重启操作，观察可见 Tab、输出、输入回显、调试状态、进程存活和持久数据。

这条接缝覆盖从用户操作到内置终端及真实进程的完整路径，不新增只供测试的终端宿主 API，不用直接篡改私有运行状态代替正常启动。GPUI 自动化、真实插件包和实际 Windows 窗口交互是同一产品入口的不同证据层次，不能相互冒充。

用户于 2026-10-09 确认本测试接缝及工单拆分，要求逐步执行全部工单。

### 复用的先例与辅助验证

- 复用现有原生 Run 验收：从配置和应用动作启动真实程序，验证并行输入、会话定位、停止和重启；将旧“必须安装终端包”的夹具前提改为内置终端，不删去真实进程验收。
- 复用现有原生 Rust 调试验收：点击实际控件命中区域，通过真实 Rust/PDB 和调试提供者检查断点、步进、变量、调用栈及离开确认；扩展为调试程序输入及统一面板归属。
- 复用现有原生构建与插件开发任务验收，覆盖准备输出、取消、构建失败和打包输出的统一展示。
- 复用现有执行生命周期、公开服务消费者及进程权限测试；必要的低层失败注入沿现有边界完成，用于补充主接缝难稳定复现的超时、拒绝、晚到事件和清理失败。
- 数据迁移使用隔离的真实旧格式样本，通过应用恢复入口检查结果；回归终端重排、快照、输入、选择及通用 SideTabs、滚动条行为，不断言某个内部结构或函数必须存在。

### 外部行为验收矩阵

| 编号 | 场景 | 可观察结果 |
| --- | --- | --- |
| N01 | 未安装终端包的全新用户 | 可打开内置终端、输入和退出；没有终端插件依赖或重复入口 |
| N02 | 打开空面板、关闭最后一个 Tab | 打开产生一个 Shell；最后一个 Tab 关闭后面板隐藏；任务启动不额外产生 Shell |
| N03 | Shell、构建、运行与调试混合使用 | 共用一个底部面板，文件 Tab 不增加工具页，其他底部输出块不再出现 |
| N04 | 同次准备、构建和正式运行 | 输出按步骤归入同一任务 Tab；准备失败或取消不启动目标 |
| N05 | 独立构建及插件开发、构建、打包 | 输出、错误、取消和退出状态均在统一面板可见 |
| N06 | 两个配置并行、普通 Shell 同时使用 | 各自输入输出和停止独立，不占用或改写普通 Shell |
| N07 | 同一活动配置重复运行及显式重新运行 | 普通运行只定位；重新运行等待旧进程退出后才启动一次新目标 |
| N08 | 任务结束后再次运行 | 对应 Tab 未关闭则复用并清除上次输出；已关闭则新建；旧事件不得写入新执行 |
| N09 | 活动任务关闭确认 | 取消保留原 Tab 和进程；确认停止完整任务及进程树后关闭；失败仍可见 |
| N10 | 准备中或启动中关闭、确认期间自然退出 | 无重复启动、无遗漏清理、无悬挂确认；不遗留隐藏活动任务 |
| N11 | 已结束任务关闭、面板隐藏与定位 | 已结束直接关闭；面板隐藏不停止，定位准确显示原活动 Tab |
| N12 | 切换调试 Tab 与普通 Tab | 调试按钮和断点、变量、调用栈在同一面板内部按当前会话展示，正常 Shell 输入不受干扰 |
| N13 | 真实 Rust 调试目标请求输入 | 程序只启动一次，输入到达被调试目标并回显结果，仍能命中断点和检查状态 |
| N14 | 调试暂停、步进及多会话切换 | 栈、变量与源码归属正确，旧暂停代次结果不能覆盖当前视图 |
| N15 | 调试握手失败、超时或插件退休 | 目标、PTY、适配器和订阅回收；错误明确，不降级启动普通程序 |
| N16 | 受限工作区、权限不足、伪造跨会话句柄 | 拒绝执行或访问，不借内置终端提升权限；数据仍可安全读取和迁移 |
| N17 | 不同身份的合法插件调用终端 | 经公开能力执行、输入、查询、定位和停止，不要求修改宿主插件名单 |
| N18 | 旧安装记录、运行配置及终端快照迁移 | 设置、Tab、目录、历史和布局保留，配置可执行，旧贡献不重复注册，备份存在 |
| N19 | 重复迁移、中途写入失败或损坏样本 | 不重复导入、不覆盖有效新数据、不清空原数据；错误与恢复路径明确 |
| N20 | 不同工作区及编辑器重启 | 状态互不串用；普通 Shell 按信任条件重建，旧运行和调试程序不自动执行 |
| N21 | 历史恢复后连续改变宽高 | 输出顺序保持，无人为恢复提示、重复命令行或额外空白；容量上限继续生效 |
| N22 | 输入、IME、选择、复制粘贴和右键菜单 | 真实输入与中文组合可用，空白选择限制和菜单可用状态正确，清空缓冲区有效 |
| N23 | Tab 名称、拖动、左右侧栏和滚动条 | 命名及失焦保存、排序、宽度、选中样式和无滚动后 1 秒隐藏规则保持 |
| N24 | 主题、语言、字号与面板缩放 | 中英文、深浅主题及自定义覆盖可用，控件不裁剪，焦点和光标位置正确 |
| N25 | Shell、全屏 TUI、长行与混合字符 | 常用交互、交替屏幕和宽字符显示保持；不能仅凭核心依赖存在宣称兼容 |
| N26 | 窗口关闭或切换项目 | 取消保留原任务；确认完成清理；恢复输出不会再次启动已停止任务 |

### 实施阶段验证要求

- 先运行受影响模块和上述集成测试，再执行仓库规定的 Rust 格式、非 UI workspace 测试、workspace 编译检查与相关应用测试。
- 真实语言、调试器及消费者包通过宿主公开 SDK 入口独立构建；按实际夹具要求显式执行 ignored 测试。退役终端包后修正相关测试前提，不能通过跳过测试绕开内置化验收。
- 实际 Windows 原生操作必须覆盖交互输入、调试 stdin、Tab 生命周期、主题、IME、滚动和 resize。其他平台按真实可用环境分别记录编译与运行结果，不把 Windows 通过写成多端全部通过。
- 构建、打包和 SDK 验证遵循当前直接 Cargo、宿主构建入口及归档方式，不调用已移出仓库的旧脚本，也不自动安装缺失编译器或系统 SDK。
- 本次仅生成规格：检查需求一致性、文档索引、链接及差异格式；不运行无关 Rust 构建，不引用历史通过记录宣称本方案实现完成。

## Out of Scope

- 用户已要求执行四张工单，按仓库既有授权测试与审查后普通提交、推送并关闭对应工单；不关闭历史父议题，不强制推送、重写历史或发布软件版本。
- 将语言插件、Rust 构建规则或具体调试器全部内置；为单个插件新增专属宿主 API。
- 新增调试表达式求值 REPL、附加调试、远程调试、组合配置、跨项目任务编排或脱离编辑器继续运行。
- 将历史 Shell 的进程内存、临时变量、活动命令或调试执行位置原样恢复；自动重跑历史命令。
- WebView 终端、另起一套主题或停靠系统、终端核心私有 fork、实现全部高级图形或键盘协议。
- 为通用 Windows 顶部或左侧 resize 合成现象再次修改 GPUI 或扩大窗口平台修复范围。
- 清理无关插件数据、全局设置或其他工作区；未验证平台的兼容性保证。

## Further Notes

### 依据与替代关系

- 来源为本轮 grilling 确认：恢复 Alacritty、采用统一面板、支持调试程序输入、保留语言/调试器插件边界、活动任务 Tab 提示停止关闭、未关闭 Tab 复用及已关闭后新建；用户随后采纳完整整理并要求生成方案。
- [插件平台规格](plugin-api-platform.md)、[运行、调试与构建规格](run-debug-build.md)、[既有调试器握手决定](run-debug-build-debugger.md)、[运行配置规格](run-config-plugin-tree.md)和[插件 UI 解耦规格](plugin-ui-decoupling.md)提供未被本方案替代的基线。
- [运行与调试终验](../verification/run-debug-build-completion-2026-10-06.md)及当前实现证明已有真实运行和调试基础；旧文档中的进度与旧终端包版本不是当前实现状态。本方案没有继承任何历史验收完成标记。
- 采用现有领域词汇：宿主、插件包、能力接口、提供者、实例、服务契约、资源句柄、状态快照、运行配置、执行会话、调试会话和暂停代次。本轮未发现额外独立 ADR 或领域上下文文件。
- 跟踪器旧地址 T-miracle/Editor 已由 GitHub 返回为 T-miracle/Nanobug；发布使用现行仓库，历史父议题不改写或关闭。

### 发布与后续工作

用户已确认测试接缝与四张工单，并明确要求逐步执行全部工单。按稳定标识查重，发布完整正文并设置 `ready-for-agent`，登记实际 issue URL 与原生阻塞关系；按 01 → 04 实施，不修改或关闭历史父议题。每张工单验收和审查后普通提交、推送并核对关闭，未完成项不得标记完成。

后续实施需同步更新终端相关发行说明、受影响 SDK 与读者文档，并将真实验收另行归档；不在本方案中预先勾选完成。具体协议编码、内部模块拆分和调试器 PTY 握手细节由实施阶段在既定产品与权限边界内验证，不以技术细化为由重新要求用户决定可由代码证据解决的问题。

<!-- builtin-terminal-run-debug:spec:2026-10-09 -->
