# 运行、调试与构建：整批审查与接手记录（2026-10-05）

历史初审结论：已有配置模型、执行服务、准备序列、原生界面和测试基础可以复用，但初审时整批尚未达到 #48 的交付标准。缺口包含真实产品行为，不能归结为只差输入法、调试器或关闭确认。本文保留 2026-10-05 的审查、第一阶段修复及后续中间记录；“尚未修复”“进行中”和未关闭状态均属于当时结论。

当前实现、终验与工单处置统一见 [2026-10-06 终验记录](run-debug-build-completion-2026-10-06.md)，不以本文历史表作为当前待办清单。

## 范围与依据

- 用户确认按整批工单审查。固定起点为 `e0f69d3b51032a89812db47a28fea3ec7c0272fa`，DeepSeek 交付 HEAD 为 `cfe130763dff1549d61bee1ec1d42cc026133865`；原批次共涉及 83 个文件。当前候选修复另以未提交工作区 diff 核对。
- 工作区为 `C:\Projects\RustProjects\Editor-run-debug-build`，本地分支已按用户要求命名为 `Editor-run-debug-build`。它仍跟踪原远端分支 `origin/codex/run-debug-build`；本轮未提交、推送、合并或修改 GitHub 工单。
- 主工作区 `C:\Projects\RustProjects\Editor` 的既有修改保留。本轮也未改动 `Editor-docs-site`。
- 产品依据是 [#48](https://github.com/T-miracle/Editor/issues/48) 及 #49–#60 正文，另核对本分支的 [需求整理](../../需求整理.md)、[开发计划](../../开发计划.md)、[插件平台规格](../../specs/plugin-api-platform.md)、[工单授权](../../agents/issue-tracker.md) 和用户本轮提供的仓库约定。
- 本分支仍使用部分历史文档路径，不能据主工作区的新路径不存在就推断规格或站点不存在。历史验收记录只作为待核对证据，不替代规格。
- Standards 与 Spec 分别由并行审查代理只读审查；本节两轴保留初次报告各自的顺序。追加检查和候选修复复审另作说明。

## Standards

### 1. [P1] 宿主会话入口借用了宿主权限——本阶段已修复

原 `session.host` 各方法权限为空，转发启动时重建有权限的宿主 caller，丢失真实来源的权限与撤销链。只有服务调用权限的插件可能借此启动程序，违反授权交集和实例资源归属。

[host_services.rs](../../../crates/plugin-runtime/src/manager/host_services.rs) 现在保留原始 `CallContext`，并在启动、后续查询和停止时委派相应方法权限；[plugin_services.rs](../../../crates/plugin-runtime/src/plugin_services.rs) 的通用委派保留来源及提供者存活链。`start` 要求 `process.exec`、`ui.panels`，`stop` 要求 `process.exec`，读取只能读取自己的会话。

权限要求是契约破坏性变更，因此 `session.host` 升为 **2.0.0**。旧 `^1` 消费者需更新声明，不增加绕过权限的兼容分支。测试消费者与 [SESSIONS.md](../../../crates/plugin-protocol/SESSIONS.md) 已同步；`interactive.execute` 保持 1.3，未修改终端包业务代码或清单版本。

回归验证来源缺权、来源退休、跨消费者管理拒绝。真实 SDK 访客测试验证未声明启动方法的消费者被拒绝且没有新增进程；该真实用例的拒绝是 `UnsupportedOperation`，来源权限检查的 `PermissionDenied` 证据来自就近测试，不混淆两者。

### 2. [P1] 会话容量能淘汰活动进程——本阶段已修复

原容量逻辑把启动请求已完成当作可淘汰条件；这仅代表程序创建成功，仍可能正在运行。全为未完成启动时也没有可靠拒绝边界。结果是程序继续运行，却失去宿主管理记录。

现在先预留容量再入队；最多保留 64 项，只回收 `Exited` 或 `Failed`，全部活动时拒绝新启动。宿主以原提供者的 `status` 观察结束，不用停止回执、日志或延时假定退出。

复审发现“启动后立即关闭终端页签”会令提供者永久返回 `InvalidHandle`。已补来源及提供者一致性检查，把明确丢失的会话记为失败并保留诊断，停止自动查询，避免容量泄漏；不把丢失会话写成成功退出。普通临时查询失败不能说明程序结束。

实际包回归覆盖自然退出 65 次、提前关闭终端、来源撤销及原提供者停止；重建包复跑结果见验证表。

### 3. [P1] 会话没有工作区与来源可见性隔离——本阶段已修复

原共享表查询没有按工作区和 caller 筛选，切换工作区后能暴露原工作区记录，其他消费者也可能管理别人的会话。

`visible_to` 现在要求同一逻辑作用域；普通消费者只看到原实例自己创建的会话，宿主窗口可以管理当前工作区的会话。停止与查询固定到启动时的提供者 incarnation；改变默认选择只影响后续启动。已经结束的停止保持幂等。

来源、作用域与容量就近回归通过；实际包的“切换默认后查询和停止旧会话，新启动使用新提供者”已通过。调试会话的同类绑定仍需另补，不能把执行会话修复推广为调试已完成。

### 4. [P2] 下拉控件、键盘和国际化未遵守本地 UI 契约——尚未修复

[run/ui.rs](../../../crates/editor-app/src/run/ui.rs) 仍直接使用 `KitPopupMenu`，并存在大量写死中文的可见文本。本地控件和 i18n 迁移不足。

[run_ui_tests.rs](../../../crates/editor-app/src/run/run_ui_tests.rs) 的键盘用例断言弹层未获得焦点、Escape 未关闭；它记录障碍，但不是键盘选择验收通过。需在本地 UI 接缝修正焦点和键盘行为，再验收中英文、主题、缩放和窄窗口。本轮新增修复动作与错误文本使用已有 i18n 资源，未宣称完成整个 UI 迁移。

## Spec

### 1. [P1] 调试启动与基础控制没有通过 worker——尚未修复

[extensions/worker/runner.rs](../../../crates/editor-app/src/extensions/worker/runner.rs) 的 `Work::DebugCall` 只处理 `frames`、`variables`、`step`、`set_breakpoints`，其余方法被拒绝。界面发送的 `start`、`resume`、`pause`、`stop`、`status` 没有贯通。

仓库也没有实际 Rust 调试提供者。测试把普通示例包重新声明为调试提供者，只能验证声明与能力展示，不能验证真实断点。#57/#58 需要实现提供者和调试流程；仅安装调试器不会修好这条链路。#57 要求先记录调试器、适配协议、传输、授权及执行/调试握手的选择，当前缺少已定稿方案。

### 2. [P1] 调试绕过保存及启动前准备——尚未修复

[run/ui.rs](../../../crates/editor-app/src/run/ui.rs) 的调试启动直接发送 debug start，没有复用 `save_dirty_documents` 和准备序列。[RunControls::debug_launch_request](../../../crates/editor-app/src/run.rs) 只取准备计划最后的程序。

即使补一个可用调试器，也会跳过脏文档保存、构建和用户准备动作，可能调试旧产物。应让运行与调试共用准备和失败阻断规则，并明确目标只启动一次的握手。

### 3. [P1] 发现配置把最终命令复制为准备步骤——本阶段已修复

原 [discovery.rs](../../../crates/editor-core/src/run/discovery.rs) 对 Cargo 命令有宿主专属推断，并把 `cargo run` 放进准备和最终程序：长驻程序阻止后续启动，短程序可能运行两次。

现在只使用提供者声明的最终程序与构建字段；本配置构建已经由通用准备序列执行，因此默认 `prelaunch` 留空。新回归从确认候选进入实际 `launch_plan`，验证构建一次、最终运行一次。候选修复中一度使用的自身构建引用与现有按名称引用规则不兼容，已在交付前回归发现并撤除。

这只修复重复执行；真实 Rust 产物发现仍见追加问题 8。

### 4. [P1] 共享配置导出个人绝对路径——本阶段已修复具体路径缺陷

原 [shared.rs](../../../crates/editor-core/src/run/shared.rs) 直接复制最终程序、准备动作和外部目录；断点源码也能携带个人绝对路径。

项目内路径现在转为 `${workspace}` 形式，外部路径、未知变量、目录穿越等拒绝共享；项目相对工作目录解析到当前项目。覆盖可执行路径、字面路径参数、`--option=path` 参数值、构建/启动前动作和断点源码，保留原 argv 边界。个人环境、工具目录、提供者和授权仍留本机。

脚本正文是用户代码，不由宿主解析；任意普通文字参数是否含秘密也不能由路径检查判定。以上修复不构成“自动扫描所有敏感内容”的承诺，也不替代 #54 的完整界面及跨项目验收。

### 5. [P1] 本机选择能覆盖手工共享修改，错误文件会执行缓存——本阶段已修复

原选择保存会把缓存共享集写回 `runs.json`，覆盖外部编辑；共享读取失败时还能采用本机镜像里的旧项目命令。

现在选择只写本机偏好，显式共享编辑才读取当前文件并修改目标条目。缺失或损坏的项目定义不能由缓存复活；运行、构建和调试计划在新启动前读取当前共享文件，并在同一快照上展开准备。

保存先验证共享提案，再保存本机数据，最后提交共享文件。本机失败不发布共享命令；共享失败回滚本机数据；若回滚再次遇到 I/O 故障，明确报告双错误，不冒充跨文件原子事务。删除和取消共享采用同一路径。运行中的会话继续使用启动快照，重复点击先定位旧会话，不因项目文件暂时损坏而失去管理入口。

回归覆盖重新打开、同窗手工修改、格式错误、删除、选择保留文件原字节、本机写入失败、共享写入失败及活动会话定位。

### 6. [P1] 重发现未经确认替换用户程序——本阶段已修复

原重发现自动调用 `repair` 修改存储的 executable，与 #55 要求确认更新冲突。

现在发现只报告候选变化；统一菜单显示具体“配置 → 新程序”的确认动作，用户点击后才调用 `repair_target` 和正常持久化。程序以外的用户配置保留。回归验证重发现只报告、不自动改程序，明确修复后才改变目标。

### 7. [P1，追加] 正常退出、立即终止与超时升级没有区分——尚未实现

[terminal/service.rs](../../../plugins/terminal/src/service.rs) 的 `stop_call` 直接调用 `Process::Terminate`；现有执行/宿主 stop 参数只有会话 ID，无法表达正常退出、立即强制和超时升级策略。

[#50](https://github.com/T-miracle/Editor/issues/50) 和 [#56](https://github.com/T-miracle/Editor/issues/56) 明确要求这些行为及对应程序验收。旧记录把正常停止和立即终止一并列为通过，缺乏代码依据。修正曾经排空工作队列的测试误诊是正确的，但不能据此认定 #50 全部合格。

### 8. [P1，追加] Rust 发现没有完整解析目标与构建产物——尚未实现

[历史 rust.json](https://github.com/T-miracle/Editor/blob/cfe130763dff1549d61bee1ec1d42cc026133865/plugins/rust/run-targets/rust.json) 主要提供 `cargo run`/`cargo build` 默认字段；普通 `[[bin]]` 在 TOML 中是数组，[run_targets.rs](../../../crates/plugin-schema/src/run_targets.rs) 的逐键读取不会遍历该数组。多目标参数、构建模式、实际二进制产物都没有形成完整发现结果。

[#55](https://github.com/T-miracle/Editor/issues/55) 要求 Rust 插件解析目标、模式和产物，并运行真实程序。需要补插件领域逻辑，通过通用接口交付；不能在宿主增加 Rust 分支。单一包上 `cargo run` 成功不证明多目标与产物调试已满足。

### 9. [P1，追加] 调试控制请求与会话身份关联不完整——尚未修复界面调用

[run/ui.rs](../../../crates/editor-app/src/run/ui.rs) 的暂停/继续/停止用请求号 `0` 发送，未登记能消费成功回复的待办；`step_debug` 发送配置 ID，未使用提供者的 session ID。

本轮先修复模型层的请求号复用和跨会话晚到数据：请求号单调递增，待办记原配置，帧/变量不能写到另一会话；单步响应按原配置更新其状态，切换选择不丢失已发生的状态变化。这些回归通过，但不等于上述 UI 调用已经接通。

### 10. [P1，追加] 通用调试调用的匹配、等待与归属仍有缺口——尚未修复

[debug_services.rs](../../../crates/plugin-runtime/src/manager/debug_services.rs) 展示能力时只检查必需方法，但调用用的 dependency 保留全部九个方法；`Dependency::matches` 要求每项均匹配，所以被宣称可选的检查/单步方法实际影响基础调用的解析。

该调用声明 15/60 秒窗口，却调用只等 500 ms 的 `poll_request`；未完成请求会被报告失败，仍可能随后产生副作用而没有可消费的完成记录。此外，每次调用重新按当前默认解析，未固定原调试提供者 incarnation。需要异步请求关联和调试资源归属模型，覆盖慢启动、默认切换、重启及迟到结果，而非延长 UI 阻塞等待。

### 11. [P1，追加] 公开会话订阅缺失——尚未实现

`session.host` 当前仍只有 start/list/status/stop；缺少 [#56](https://github.com/T-miracle/Editor/issues/56) 指定的输出、状态订阅及有界撤销语义。本轮自动状态观察供统一快照使用，不能冒充公共订阅。

订阅已经在工单范围内；不能用删判据换取完成。#51 交互输入则不同：终端 `send` 已通过公开 Process 写入它拥有的执行进程；缺的是交错输入、归属、隐藏恢复的行为验收，不能仅因 execute 服务没有 stdin 方法就判断输入能力不存在。

## 工单实际状态与剩余工作

2026-10-05 从 GitHub 读回：#49、#50、#51、#53、#55 已关闭；#52、#54、#56、#57、#58、#59、#60 共 **7** 张打开。关闭状态只代表跟踪器状态。

| 工单 | 仍需处理 |
| --- | --- |
| #49 | 基础运行路径已存在；本轮权限/归属修复须纳入其回归，不据此宣称整批通过。 |
| #50 | 实现正常退出、超时升级、立即终止及对应程序/进程树验收。旧测试误诊不再作为重开理由，但新的判据缺口仍成立。 |
| #51 | 修正下拉焦点与键盘选择；验证两个程序交错输出和输入、输入归属、隐藏后恢复与单独停止；订阅接缝仍待补。 |
| #52 | 原生 IME 和候选窗口实测；已有组合协议测试不能替代真实输入法。 |
| #53 | 保留独立构建及序列基础；本轮重建包复跑准备期间停止，正常停止策略仍依赖 #50/#56。 |
| #54 | 已修具体泄漏、缓存和保存故障；仍需完整共享 UI、隔离项目实际运行与私有覆盖验收，不应直接关闭。 |
| #55 | 补真实 Rust 多目标/构建模式/产物解析，实测目标改名、删除及确认修复。 |
| #56 | 权限、来源退休与执行提供者绑定已修；补有界订阅、正常/强制终止、消费者展示输入和 SDK 契约验收。 |
| #57 | 先定稿调试技术选择与握手，补真实提供者、基础调用、保存准备和资源清理，再验收断点与控制。 |
| #58 | 依赖 #57；接通正确请求/提供者会话身份，验收真实步进、调用栈、局部变量、多会话和滚动等交互。 |
| #59 | 普通来源撤销与回收已有新增真实包证据；断点暂停清理和故障原生提示仍依赖真实调试及交互验收。 |
| #60 | 完整 R01–R20、读者站点与 SDK 交付；不能用声明夹具替代真实调试演示。 |

`website/` 在 `Editor-docs-site` 工作区确实存在，只是尚未进入本分支。应作为后续分支集成和读者文档交付项，不记录为“仓库没有站点”。本轮遵循隔离要求，没有跨工作区合并或在 `docs/` 新复制读者正文。

后续顺序：先完成 #50/#56 的终止与订阅基础、#51 的本地 UI 交互及 #55 的实际 Rust 目标，再完成 #57/#58 的真实调试链路，最后补 #59/#60 的组合、原生和读者文档验收。已关闭但尚缺判据的工单需要补实现与证据，不能把工单关闭当作技术结论。

## 第一阶段修复复核

本轮候选修复经过多次只读复审。复审发现的“单步回复切换后丢失”“先写共享后本机失败”“断点路径遗漏”“同窗仍用旧定义”“活动会话被共享错误挡住”等问题已先补失败回归再修正；这些候选问题不归因于 DeepSeek 原始提交。

重建终端包后，原生构建组合验收还发现本轮引入的状态次序回归：`Exited` 快照早于专用查询的退出码回复，准备序列曾误报“会话已结束但未报告结果”。已将“仍活动”和“仍能取得结果”分开：可查询的 `Exited` 等待实际退出码，只有零退出码推进；非零、无退出码和结果来源失效均阻断。就近用例先复现失败，再修正，并由原审查代理定向复核；原失败日志保留为 `target/native-build-red.log` 和 `target/preparation-exit-code-red.log`。

新增代码均附用途、权限、状态或生命周期注释；未放开公开字段、增加插件 ID 特判或引入测试专属宿主 API。

SDK 导出补入原先 README 已链接、实际未随 SDK 导出的 `SESSIONS.md`；导出、损坏修复和仓库外独立组件构建已实测。终端包随后经同一公开宿主入口重新构建；重建后的 WASM 与原夹具字节不同，因此实际包回归继续复跑，不沿用旧包的通过结论。

## 验证记录

完整日志留在本工作区 `target/`，未提交构建日志和产物。以下数字只表示相应命令的结果。

| 命令 | 本轮结果 | 日志 |
| --- | --- | --- |
| `cargo fmt --check` | 通过 | 命令无错误输出 |
| `cargo test --locked --workspace --exclude editor-app` | 169 passed，0 failed，133 ignored；跳过不计通过 | `target/workspace-review-tests.log` |
| `cargo check --locked --workspace` | 通过；有既有编译警告 | `target/workspace-check-final.log` |
| `cargo test --locked -p editor-app run:: -- --test-threads=1` | 86 passed，0 failed，0 ignored | `target/app-run-final.log` |
| `cargo build --locked -p editor-app` | 通过 | `target/editor-app-build-final.log` |
| `./scripts/verify-plugin-sdk.ps1 -HostExe ./target/debug/editor-app.exe` | 公开 SDK 导出、修复、独立组件构建通过 | `target/sdk-verification-final.log` |
| `./scripts/build-plugins.ps1 -HostExe ./target/debug/editor-app.exe -Packages terminal` | 当前终端 WASM 与 ZIP 构建通过 | `target/terminal-package-final.log` |
| `cargo test --locked -p plugin-runtime --test host_execution -- --ignored --test-threads=1` | 重建包 15 passed，0 failed，0 ignored | `target/host-execution-final.log` |
| `cargo test --locked -p plugin-runtime --test interactive_execution -- --ignored --test-threads=1` | 重建包 9 passed，0 failed，0 ignored | `target/interactive-execution-rebuilt.log` |
| `cargo test --locked -p editor-app extensions::native_build_tests -- --ignored --test-threads=1` | 当前包 5 passed，0 failed，0 ignored；顺序回归修复后全组复跑 | `target/native-build-final.log` |
| `git diff --check` | 通过 | 命令无错误输出 |

就近回归已实测 RED → GREEN；日志包括 `target/additional-app-red.log`、`target/early-terminal-close-red.log`、`target/breakpoint-portability-red.log`、`target/shared-options-red.log` 和 `target/active-shared-red.log`。测试驱动修正了无原生窗口循环时积累未应答面板请求的夹具限制，以公开请求 begin/finish 应答；没有从工作队列偷取产品任务。

一次并发重跑曾出现 Windows `LNK1104`：运行中的测试二进制不能被重新链接。结束对应运行后复跑已通过；不把该构建碰撞归为产品缺陷。原输入法、真实断点/调试进程树、滚轮和人工崩溃提示仍未在本轮完成，不将其计为通过。

## 严重度与交付状态

- Standards：4 项，P1 3、P2 1；P1 三项完成本阶段修复，P2 控件/交互问题保留。
- Spec：11 项，P1 11；初次报告的第 3–6 项完成具体缺陷修复，其余 7 项仍待实现或贯通。
- 本阶段修复可以独立审阅；整批未完成，不关闭任何议题、不修改原父设计、不发布或合并主分支。

## 接手补全：公开会话与真实调试（进行中）

以下为后续实现的实测增量，不能把前一阶段测试数字当作当前 HEAD 全量结果。修改仍在独立 `Editor-run-debug-build` 分支，未合并到主工作区。

- `process` 1.5 区分正常退出请求和显式强制终止；宿主等待实际退出，保留终端句柄、最终退出码和 Force 来源。真实正常清理、忽略中断后强制、父子进程树、开始中停止、离开与切换清理共 6 项通过：`target/stop-current-final.log`。
- `interactive.execute` 2.0 补 `input`、`locate`、有界输出／状态事件；`session.host` 2.0 补来源所有权、订阅、单次消费和撤销。13 项公开会话验收在 `target/public-sessions-complete.log`（12 通过）与修正声明夹具后 `target/session-authority-current.log`（剩余 1 通过）完成；未把旧失败夹具计为通过。
- 原生终端双程序交错输入、隐藏重开和单独停止通过：`target/native-interleaved-green.log`。统一菜单使用本地 `gpui-base` 行为实现，真实 Down / Enter / Escape 焦点路由通过：`target/run-keyboard-green.log`。
- `plugin.services` 1.1 有界延后回复保留来源、截止时间和退休清理。陷阱导致的真实错误先完成原调用，再退休来源，避免被替换为取消：`target/service-trap-green.log`。公开订阅、输入取消／父期限与创建等待脱离分别见 `target/public-subscription-green.log`、`target/forwarded-cancel-deadline-green.log`、`target/cancel-wait-green.log`。
- 新增独立公开 SDK `rust-debugger` 包，安装私有固定 CodeLLDB 1.12.3 与原生字节桥。真实 MSVC Rust/PDB 的断点、步入／步过／步出、调用栈、局部变量、继续、原生 Pause、停止与适配器树释放通过：`target/debug-pause-public-green.log`（1 项，13.76 秒）。
- 并行只读审查发现暂停状态污染、旧 frame 引用跨暂停和超限回复静默超时。公开 `debug.session` 1.1 增加单调 `pause`，暂停操作必须带预期 epoch；失败控制保留真实暂停，大回复明确失败。无效步进先 RED（`target/debug-invalid-step-red.log`），修复后真实调试 GREEN；原生 DAP 故障仪器的合法步进拒绝和大变量显式 `LimitExceeded` 通过：`target/debug-fault-bounds-green.log`。故障仪器不作为真实调试证据。

当前仍在补：原生调试界面完整验收、多个真实调试会话与暂停清理、Rust/Cargo 多目标产物发现、原生 IME 候选窗口、读者站点及最终 SDK／发行检查。尚未完成的判据不计通过；工单关闭与推送在相应行为验收完成后执行。
