# 宿主消息来源盘点

日期：2026-10-07

对应：[宿主消息窗口规格](../specs/host-messages.md)、[工单 01 / GitHub #85](../tickets/host-messages/01-history-panel.md)。

状态：对 `main`（`ae63225`）及本次实现工作区进行来源复核，并完成本文所列宿主运行准入回归。本文记录覆盖、排除依据和当前生产路径，不代替整个消息功能的 Windows 原生验收或工单关闭证明。未提交。

## 来源规则

规格要求收录宿主面向用户的操作结果、警告和错误；等级由来源明确指定。插件消息，包括宿主代为报告的插件相关日志，仍属于插件日志管理。光标、输入 revision、实时进度和内部调试输出不进入宿主消息历史。

来源按实际职责判断，不按文件夹名判断：原生运行配置的本地写入属于宿主操作；同一运行 UI 显示的插件发现、准备、执行或调试回执属于插件结果。不能扫描状态栏文本推断来源或等级，也不能因暂未接入而缩小规格范围。

生产入口为 `record_host_message` 与 `report_host_message`：前者保留历史且保留现有状态栏/即时提示语义，后者同时更新状态栏与历史。重绘、消息面板打开和列表展开不回读状态字符串制造新记录。

## 已接入的来源

| 来源与具体分支 | 等级 | 记录时机与边界 |
| --- | --- | --- |
| 文档 `open_file_with_navigation` 新建文本 tab | Info | `DocumentSession::open` 成功并完成 tab 激活后记录一次；已有 tab 切换、保存标签恢复不记录 |
| 文档新建只读/非文本 tab | Info | 新建文件身份并激活后记录一次；不把缺少插件查看器的持续空态当作重复消息 |
| 文档打开失败 | Error | 原有 `status.open_failed` 分支保留即时状态并进入历史；非文本编码转文件 tab 不冒充失败 |
| `save_current` 实际保存成功/失败 | Info / Error | 沿用同一 `DocumentSession` 与文件存储结果，成功后才记录保存；失败仍保留当前文档 |
| `save_current` 无文件或无修改 | Info | 只在用户显式保存的结果分支记录；输入变化不记录 |
| 保存前磁盘冲突/删除确认 | Warning | 根据实际 `DiskState` 与读盘结果记录，保留再次保存才覆盖/恢复的规则 |
| `close_tab` 脏文档拒绝关闭 | Warning | 在保留 tab 的拒绝分支记录一次 |
| `close_tab` 成功关闭 | Info | 删除指定 tab 后记录其目标路径；随后激活相邻 tab 不再生成打开历史 |
| `apply_reconciliation` 文档磁盘状态转变 | Info / Warning | 仅在状态改变时记录；冲突和删除为 Warning，恢复同步为 Info，包含后台 tab 与文件路径；相同状态的再次扫描不记录 |
| 用户显式工作区刷新完成 | Info | 原生刷新操作置 `host_refresh_pending`，后续核对消费一次；启动刷新和自动 watcher 扫描不记录 |
| 文件树创建目录/文件、重命名成功 | Info | 实际文件系统操作成功后记录目标；内部刷新只更新树与即时状态，不再重复写历史 |
| 文件树编辑失败、打开文件阻止重命名 | Error / Warning | 已打开文件的安全拒绝根据条件明确为 Warning；其余失败来自该次文件操作结果 |
| 文件树复制及特殊复制 | Info / Error | 实际剪贴板写入完成/失败时记录；复制菜单显示不记录 |
| 文件树粘贴 | Info / Error | 成功记录一批条目与目的目录；失败保留原有失败消息，不逐文件制造重复成功记录 |
| 文件树删除 | Info / Warning / Error | 只在明确确认后的实际删除记录成功；已打开文件的安全拒绝为 Warning；文件系统删除失败为 Error；预览和取消不记录 |
| 文件树 `reveal_active_file_in_explorer` 定位失败卡片 | Info | 沿用原有“informational failure”定义；非模态卡片过期/关闭后结果仍留在宿主历史；保留原有焦点行为 |
| 当前原生配置窗口 `finish_plugin_commit` 本地提交 | Info / Error | `commit_configuration_set` 真正写入宿主本地存储后记录一次；IO/快照保存失败保留窗口与原有行内错误并记录 Error。保存成功只表示本地数据已保存，不表示提供者业务校验成功或程序已执行 |
| 当前原生配置窗口应用树快照失败 | Warning | 在本地 `apply_tree_configuration` 拒绝时记录，保留原有行内错误；不把草稿的树编辑、字段变化与取消记为保存结果 |
| 保留的历史表单 `commit_run_form`、`reject_run_form`、步骤 Done、删除/选择 | Info / Warning / Error | 旧 B3 表单仅为 `cfg(test)` 保留呈现路径，不是当前产品配置入口。原有方法内的本地结果也接入历史，但不以这些测试代替上面当前配置窗口的提交覆盖 |
| 运行菜单目标确认与显式修复/重新绑定 | Info / Error | 记录宿主本地配置确认或修复的完成/失败，不复写提供者的发现日志 |
| `apply_provider_choice` 本地配置写入失败 | Error | `RunControls::choose_provider` 写宿主配置的失败进入历史；尚未请求提供者执行 |
| `guard_plugin_execution` 不支持旧身份、受限工作区 | Warning | Run、Build、Debug 的真实通用守卫在请求提供者前拒绝旧配置身份或宿主信任状态；每次显式操作记录一次，不开放旧执行/持久化兼容路径 |
| `accept_plugin_execution` 信任变化、配置快照已变化 | Warning | 等待验证期间的信任撤销或目标快照变化由宿主拒绝，保留拒绝原因；不把提供者业务验证失败复制进历史 |
| 通用运行路由的缺少配置/受限/`plan_launch::Invalid` 本地守卫 | Warning | 保留其明确本地拒绝接入；当前产品先经过上面的通用插件配置准入，不能用一个旧 DTO 夹具声称已直接进入准备计划 |
| 调试启动缺少本地配置、受限、已有普通运行阻止调试 | Warning | 按原生宿主条件拒绝，不把调试服务的返回结果改写成宿主结果 |
| 未知宿主调试动作、缺少本地调试会话 | Warning | 在确定的本地守卫分支记录；提供者能力与连接状态的拒绝继续按插件边界处理 |
| 冻结已接受的调试计划失败 | Error | `begin_debug_preparation` 的本地状态/序列化失败记录一次，不记录“正在准备” |
| 运行/构建/发现被工作区信任拒绝 | Warning | 只在用户动作到达宿主信任守卫时记录；不启动插件或语言工具 |
| 发现请求标识分配失败 | Error | `begin_discovery` 本地分配失败记录；正常分配和发现进度不记录 |
| 运行前保存脏文档 | Info / Warning / Error | 后台文档实际保存结果与原生保存使用相同消息目的地；磁盘确认拒绝为 Warning；保存失败已记录具体错误，外层“运行前保存失败”状态不再重复写历史 |
| 主题切换 `toggle_theme` | Info | 主代理已在真正切换并保留原有状态栏结果后记录历史；没有把每次主题读取或渲染当作新消息 |

代码依据：[文档操作](../../../crates/editor-app/src/editor/documents.rs)、[文件树操作](../../../crates/editor-app/src/explorer.rs)、[文件树定位](../../../crates/editor-app/src/explorer/interaction.rs)、[文件树菜单](../../../crates/editor-app/src/explorer/menu.rs)、[通用运行路由](../../../crates/editor-app/src/run/ui.rs)、[当前配置提交与执行准入](../../../crates/editor-app/src/run/ui/plugin_form/actions.rs)、[保留的历史表单验证](../../../crates/editor-app/src/run/ui/configuration/editor.rs)、[保留的历史表单删除/选择](../../../crates/editor-app/src/run/ui/configuration/actions.rs)。

## 排除项及依据

| 具体来源 | 原因 |
| --- | --- |
| 文档 `InputEvent::Change` 的 `status.modified`、光标位置、IME、revision | 持续编辑状态；规格明确排除高频状态，不能把每次输入当作操作结果日志 |
| `activate_tab_with_navigation` 的 `status.opened` | 同一方法还负责标签切换、恢复和导航；真正新打开的结果已在来源分支记录，不观察每次激活 |
| `refresh_files` 启动/内部刷新、后台扫描、重复磁盘状态 | 自动状态维护，结果已由显式刷新或实际文件树写操作记录 |
| 运行“正在准备/构建/启动/等待重跑”、步骤标签和实时输出 | 实时进度或完整控制台输出，规格明确排除 |
| `sync_run_controls` 的提供者发现、准备、执行、停止、定位和调试回执 | 来自插件服务或执行提供者的业务结果，继续属于插件管理/日志及其原有运行展示 |
| `run/ui/plugin_form` 的配置类型、策略、绑定验证和提供者执行回执 | 插件拥有配置与业务策略；业务验证/协议解析失败继续属于插件。该模块内的宿主本地提交、信任及过期目标守卫已按上表单独接入，不能按模块名一概排除 |
| LSP 启动/恢复、grammar 加载、语言提供者配置错误、诊断与无定义提示 | 归属于插件语言能力或其展示；不因宿主读取/显示而复制到宿主消息历史 |
| 插件安装、更新、管理、命令调用、受控图像导入及插件文档请求 | 归属于插件功能/协议请求的结果与日志，继续沿原有插件入口；不按宿主代码发出方重新归类 |
| `tracing::warn!` 的 watcher 降级、后台读盘检查、导航请求错误等内部追踪 | 不是原有面向用户的宿主消息入口；没有直接把调试追踪重放进面板 |
| 持续显示的不可用查看器、按钮禁用说明、当前诊断计数 | 持续 UI 状态，不是每帧都应保存的新消息 |
| 布局尺寸/显隐保存、文件树展开、设置页导航、字体调整、自动定位偏好 | 控件状态直接改变，没有独立操作结果提示；避免以渲染状态或每次自动持久化伪造“保存成功”消息 |

## 工作区、设置、搜索、历史与 Git 核对

| 主题 | 当前入口证据 | 本单结论 |
| --- | --- | --- |
| 工作区打开/切换 | `resolve_startup_target` 在 GPUI 窗口创建前调用 `Workspace::open`；运行中的原生窗口没有打开/切换工作区命令 | 本单没有遗漏现存窗口内的工作区切换结果入口；显式刷新已接入。启动进程在窗口存在前失败不能伪称已显示在消息面板，也不为本单添加工作区切换功能 |
| 设置保存 | 原生设置当前即改即用；`SessionState::save` 返回 `()`，没有单独“保存设置”按钮或可见保存结果消息 | 不声称已验证设置磁盘保存成功，也不为每次持久化添一条重复消息。主题切换的明确宿主结果已接入 |
| 搜索替换 | `editor/popovers` 保留 `EditorState::search_session` 的原生搜索控件与动作；应用没有工作区搜索替换计划执行/完成结果入口 | 即时匹配/校验属于搜索状态；输入 revision 不记录。未将不存在的工作区搜索替换入口报为完成；后续若增加宿主替换结果，应使用同一生产入口而非观察文本变化 |
| 本地历史 | `LocalHistory` 用于原生/插件保存前 `snapshot_file`；初始化 `.ok()` 与快照 `let _ = ...` 没有向当前窗口发布结果；没有历史浏览/恢复操作入口 | 文档保存的可见结果已接入，但不能宣称快照失败、历史恢复或完整历史管理已覆盖。本单不把插件保存的内部快照变成宿主消息，也不扩展成历史管理功能 |
| Git | 应用、核心模块和动作注册没有 Git 状态、提交、pull、push 等现存用户操作入口 | 不存在可补接的当前 Git 操作结果来源；不因需求历史基线列有 Git 就在消息工单内实现 Git 功能 |

依据：[启动入口](../../../crates/editor-app/src/main.rs)、[原生设置](../../../crates/editor-app/src/app/settings.rs)、[会话存储](../../../crates/editor-app/src/app/session.rs)、[原生搜索接缝](../../../crates/editor-app/src/editor/popovers.rs)、[历史适配](../../../crates/platform-windows/src/lib.rs)、[核心公开模块](../../../crates/editor-core/src/lib.rs)。上述入口现状不改变“已有或后续新增的宿主用户消息应集中发布”的规格要求。

## 当前运行路径与存储边界

1. **当前执行准入**：产品从 `start_configuration` / `build_selected` / `debug_configuration` 先进入 `guard_plugin_execution`。没有 `plugin_configurations` 身份的旧 DTO 当场被拒绝；宿主信任、旧身份、等待期间快照变化及本地存储失败均已有独立宿主记录。回归没有伪造 `Bridge::resume` 或提供者验证回执。
2. **准备计划的实际归属**：旧独立原生配置入口不能越过当前守卫直接调用 `launch_plan`、`prepare_build` 或 `debug_blocker`。这些方法仍作为提供者验证成功后的通用规划机制使用：`accept_plugin_execution` 接受当前公开 `Launch` 投影，随后一次性恢复相应动作。该合法路径中的构建/预启动业务步骤、引用、提供者目标与发现缓存属于提供者准备结果；宿主代为解释这些插件相关失败仍留在原有运行展示与插件日志中。不能因为方法由宿主实现就复制整支字符串错误，也不能把整个规划机制误称为死代码。未来若新增独立宿主计划来源，应在真实来源接缝携带结构化归属，不能按文字或字符串相等推断。
3. **历史与会话底层存储失败**：当前被忽略的结果未形成原有可见消息入口，不在本单静态覆盖名单中。若以后改变这些失败的用户提示策略，新的宿主 Warning/Error 必须同步接入；不得把无返回值持久化调用当作有证据的成功结果。

## 验证说明

- 静态复核检索了宿主 `status` 赋值、通知卡片、集中消息入口、原生运行表单错误和模块公开入口，并逐分支核对实际消息归属。
- 补接完成后执行针对本次来源文件的 `git diff --check`；结果通过，Git 的 LF/CRLF 提示不是空白检查失败。
- 精准回归：`$env:CARGO_TARGET_DIR='C:\Projects\RustProjects\Editor\target'; $env:RUST_MIN_STACK='16777216'; cargo test -p editor-app host_messages_run_admission_retains_host_refusals -- --nocapture`。正确红阶段编译通过后在真实 Run 拒绝的 `host-message-1` 缺失断言失败；补接后 **1 passed / 0 failed / 0 ignored**。同一实际窗口覆盖 Run、Build、Debug × 旧身份/当前格式受限身份共六次宿主拒绝与重绘不重复。
- 当前身份夹具由 `accept_configuration_projection` 接受未校验的结构化缓存，按当前 `editor_core::save` 格式写入隔离目录并用 `load_plugin_configurations` 读回；信任拒绝发生在提供者调用前。没有恢复旧协议或伪造执行授权。
- 未构建或运行额外 WASM 配置包，不能把上述回归报为提供者正常执行、业务错误或全部配置保存 UI 的验收。文档与文件树真实成功/失败、消息容量与布局仍由统一 GPUI 应用夹具及集中验收记录证明；不得仅凭本盘点标记 #85 完成。
