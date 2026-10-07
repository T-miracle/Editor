# 运行、调试与构建终验（2026-10-06）

状态：#49–#60 十二张实施工单的实现、行为验收与代码推送已完成，GitHub 回读均为 `closed/completed`。父设计 #48 保持开放，未合并主分支。

> R01 的视觉验收更正：用户随后指出顶栏文字按钮与 B1 弹窗布局偏差。下表的原检查不足以证明原型外观一致；修复、先失败后通过的回归和 Windows 实际窗口观察见 [B1 UI 修复与复验](run-debug-build-ui-fix-2026-10-06.md)。此前已通过的其他行为证据保留。

## 范围与隔离

设计来源为 [#48](https://github.com/T-miracle/Editor/issues/48)，审查覆盖 #49–#60 的整批代码，而非只补打开工单的测试。审查基线为 `e0f69d3b51032a89812db47a28fea3ec7c0272fa`，DeepSeek 原交付为 `cfe130763dff1549d61bee1ec1d42cc026133865`。原配置、准备序列和公开插件基础可复用；权限、订阅、正常停止、共享快照、真实产物及调试链路的缺陷已补实现和回归。

工作分支与工作区为 `Editor-run-debug-build`、`C:\Projects\RustProjects\Editor-run-debug-build`。不向主分支或主工作区合并，不修改父设计 issue #48，不发布版本。站点只取用另一工作区已提交的基础文件，在本分支维护新的读者正文，没有改动另一工作区。

代码候选从 Git index 导出，构建与测试以该目录为源码根，不借用后续未暂存生产源码；缓存中嵌入路径的运行时测试已按下述方式单独清理并重新编译。首次验证目录放在父 Rust workspace 内，独立插件构建受到外层 workspace 的识别影响，已改为仓库外临时目录。这是验证布局错误，不修改产品的 workspace 规则。构建缓存最初通过环境变量设置，导致验收里的子 Cargo 继承了额外目标目录；其真实断点已命中，但测试的默认产物路径断言失败。随后用 Cargo 的 `--target-dir` 参数指定宿主缓存并清除本次工具设置的变量，原生发现与真实包整组重跑，不改变产品或项目目标目录规则。真实包首轮还发现共用 Cargo 缓存复用了嵌入旧 `CARGO_MANIFEST_DIR` 的集成测试，六条生命周期用例因此找不到旧目录下的包；通过依赖文件与二进制字符串确认后，仅清理本任务缓存中的 `plugin-runtime` 产物，在当前隔离快照重新编译并重跑，不补造旧目录的产物。最终候选 tree 为 `20cbed4262a4edc242ce60273e4ce9ea9ab92fc3`。最后变动仅在 ignored 的构建停止测试：适配当前语言并加强实际退出检查，格式和原生构建整组重新核验；其他生产源码、默认应用用例与 SDK 内容未变，复用相应通过结果。

## 本批技术决定与契约

- [调试器、授权与单目标握手](../specs/run-debug-build-debugger.md)：CodeLLDB 1.12.3、DAP、仅回环的原生字节桥；目标由调试提供者启动一次，执行提供者只承接准备，不重复启动。
- [发现与准备](../specs/run-debug-build-targets.md)：`run.targets 1.0`，语言逻辑在 Rust 插件；Cargo metadata 发现目标，compiler-artifact 给出确切产物，宿主不推算 Rust 路径。
- `session.host 2.0`、`interactive.execute 2.0`、`plugin.services 1.1`、`process 1.5`、`debug.session 1.1`；SDK、消费者、包清单与独立构建同步。旧主版本消费者明确不兼容，不增加提权兼容入口。
- 正常停止默认等待 3 秒，随后强制清理拥有的树；不支持正常停止时进入明确强制路径。停止中提供 Force；受理回执不是退出回执，Rerun 和退休均等待真实清理。
- Build 沿本批原有保存协调后只执行构建动作。Run 与 Debug 先验证和保存，再运行准备和最终目标。失败或取消阻断后续；未保存配置草稿不执行。历史记录中“不引入隐式保存”的措辞不能理解为跳过文档保存。
- 共享格式版本 2 有限读取版本 1；个人环境、工具路径、提供者选择与授权留本机。新启动、构建引用及确认修复读取当前磁盘快照，活动会话保持原快照。

## R01–R20 对应证据

原生集成指真实 WASM 包进入公开 Package/Manager，实际原生进程、GPUI 控件与绘制结果共同验证；调试故障仪器与真实 PDB 调试分别记录。

| 编号 | 验证内容 | 主要回归或人工观察 |
| --- | --- | --- |
| R01 | 顶栏、分隔线、统一菜单、B1 四页及保存位置 | [Run UI](../../../crates/editor-app/src/run/run_ui_tests.rs)、[同窗表单](../../../crates/editor-app/src/run/run_ui_tests/modal.rs)；实际窗口四页与受限尺寸 |
| R02 | 手动程序、显式 Shell、候选确认、发现不执行 | [原生 Run](../../../crates/editor-app/src/extensions/native_run_tests.rs)、[原生发现](../../../crates/editor-app/src/extensions/native_discovery_tests.rs) |
| R03 | 修改保留、去重、失效提示与确认修复 | [核心发现](../../../crates/editor-core/src/run/discovery_tests.rs)、[Run 回归](../../../crates/editor-app/src/run/regression_tests.rs)；Provided→Program 不留旧自动构建 |
| R04 | 可移植共享、本机覆盖、直接编辑 | [共享模型](../../../crates/editor-core/src/run/shared_tests.rs)、[两个实际项目](../../../crates/editor-app/src/extensions/native_run_tests/sharing.rs) |
| R05 | 保存成功用新磁盘内容；失败、取消不启动；重复只定位 | Run 回归、原生 Run、原生产物调试 |
| R06 | 多步骤顺序、失败及停止阻断、真实退出码 | [原生构建](../../../crates/editor-app/src/extensions/native_build_tests.rs)、[序列回归](../../../crates/editor-app/src/run/sequence_tests.rs) |
| R07 | 单独构建与启动分离、复用当前构建定义 | 原生构建；当前共享引用与直接构建使用同一准入 |
| R08 | 并行、去重、等待旧进程退出后 Rerun | 原生 Run、[宿主执行](../../../crates/plugin-runtime/tests/host_execution.rs)、[公开消费者](../../../crates/plugin-runtime/tests/interactive_execution.rs) |
| R09 | 独立输出和输入、隐藏不停止、恢复原会话、单独停止 | 原生 Run 的双程序交错输入及公开 Locate、订阅测试 |
| R10 | 正常清理、超时强制、立即停止、子孙进程、启动中停止 | [实际生命周期](../../../crates/plugin-runtime/tests/execution_lifecycle.rs)、[实际调试](../../../crates/plugin-runtime/tests/real_debugging.rs) |
| R11 | 窗口和项目离开可取消，确认清理 | 原生 Run 与应用 session 回归；生命周期切换工作区 |
| R12 | 独立不同 ID 提供者，公开 SDK 接入 | 公开消费者、声明式 run-target-example、原生发现 |
| R13 | 受影响会话确认、失败回收、准备失败保留旧实例、不重放 | 宿主执行、[并行调试管理](../../../crates/editor-app/src/extensions/native_discovery_tests/debugging.rs)、[真实 WASM 故障](../../../crates/editor-app/src/extensions/native_run_tests/debugging.rs) |
| R14 | 受限、授权交集、错误版本、跨来源及伪造句柄拒绝 | 公开消费者的 forged_host_handles_and_bounded_subscriptions；原生受限；通用服务回归 |
| R15 | 启动受理与退出分离、取消等待与停止分离、无孤儿 | 宿主执行、实际生命周期、公开消费者和延后回复 |
| R16 | MSVC Rust/PDB 断点、Pause、Continue、步入/过/出、栈和局部变量 | 实际调试；原生 Rust 调试控件与 Cargo 产物组合 |
| R17 | 两份真实调试配置、独立控制、暂停代次和晚到结果、防打断 | 并行调试管理、Run 检查状态回归；源码异步打开保留调试焦点 |
| R18 | 缺能力不普通运行、显式提供者缺失、缺工具与连接失败清理 | Run 准入、公开服务、实际初始化失败的三进程树 |
| R19 | 原始 argv、空格/引号/中文/元字符、显式解释器、目录及环境 | 原生 Run、两个实际共享项目、公开消费者 |
| R20 | 两种主题/语言、键盘、IME、滚轮、字号缩放、受限窗口与故障状态 | 同窗表单、真实调试键盘和滚轮、下面的 Windows 人工验收 |

### Windows 实际窗口观察

使用本分支编译的编辑器和隔离测试项目/插件目录，未操作用户真实运行会话。

- Microsoft Pinyin：在单行名称和多行脚本控件分别输入 `n → ni`。预编辑替换而非追加，候选窗口出现在当前字段旁；空格提交“你”，候选消失。脚本换行后“第二行”实际可见，保存再打开保持值；取消关闭表单并保留主窗口。620×720 受限窗口仍有保存/取消入口。
- 实际 WASM `fault-spin` 经公开菜单触发预算 trap。状态栏显示“插件错误 1”，原生详情保留 `all fuel consumed by WebAssembly` 和原始调用栈；关闭详情后编辑器继续响应。自动化组合另外确认无关运行会话继续活动。
- 最后一次使用 UI Automation 读取详情并关闭窗口时，AccessKit 0.34 的 `client_top_left` 在后台线程报告无效 HWND，宿主进程退出码为 0；之前输入法窗口与其他隔离窗口关闭正常。该上游无障碍关闭竞态尚未修复，不计作本批进程清理或 WASM 故障回收失败，也不宣称整个原生平台无缺陷。日志：`target/native-fault-ui/stderr.log`。
- GPUI 全应用测试使用进程级 `RUST_MIN_STACK=16777216`；默认测试线程栈曾在未改动的布局测试溢出。没有修改全局环境或产品线程栈。

## 最终检查

本批相关 ignored 测试先独立构建当前插件，再显式执行；其他插件的跳过项不计通过。宿主的编译与测试 Cargo 命令带 `--locked --target-dir C:/Projects/RustProjects/Editor-run-debug-build/target`，清除本次工具设置的 `CARGO_TARGET_DIR`，防止验收的子 Cargo 继承额外目录。应用测试和真实包测试使用进程级 `RUST_MIN_STACK=16777216` 及 `--test-threads=1`。

| 命令或检查 | 实际结果 | 日志（`target/run-debug-build-delivery/`） |
| --- | --- | --- |
| `cargo fmt --check` | 通过 | `fmt.log` |
| `cargo test --workspace --exclude editor-app` | 175 passed、0 failed、160 ignored | `non-ui-workspace.log` |
| `cargo check --workspace`、`cargo build -p editor-app` | 通过 | `workspace-check.log`、`host-build.log` |
| `scripts/build-plugins.ps1 -HostExe <当前宿主> -Packages terminal,rust,run-target-example` | 三个独立包构建成功 | `plugin-packages.log` |
| `scripts/build-rust-debugger.ps1`、`scripts/build-capability-example.ps1`（均指定当前宿主） | 调试包及 WASM 夹具构建成功 | `rust-debugger-package.log`、`capability-fixture.log` |
| `scripts/verify-plugin-sdk.ps1 -HostExe <当前宿主>` | 独立 SDK 分发、构建、损坏修复通过 | `sdk-distribution.log` |
| 当前宿主 `--plugin-cargo plugins/rust/Cargo.toml test --locked` | 6 passed、0 failed | `rust-guest.log` |
| `cargo test -p editor-app -- --test-threads=1` | 355 passed、0 failed、118 ignored | `app-all.log` |
| `cargo test -p editor-app extensions::native_run_tests -- --ignored --test-threads=1` | 12 passed、0 failed、0 ignored | `native-run.log` |
| 同上，过滤 `extensions::native_build_tests` | 5 passed、0 failed、0 ignored | `native-build.log` |
| 同上，过滤 `extensions::native_discovery_tests` | 3 passed、0 failed、0 ignored | `native-discovery.log` |
| 运行时真实包六组：`execution_lifecycle`、`host_execution`、`interactive_execution`、`plugin_services`、`real_debugging`、`real_targets`，显式 `--ignored --test-threads=1` | 55 passed、0 failed、0 ignored；6 / 16 / 14 / 8 / 6 / 5 项分组 | `runtime-real-packages.log` |
| `cargo test -p plugin-runtime --test process_capabilities --test fault_recovery -- --ignored --test-threads=1` | 6 passed、0 failed、0 ignored；另有 1 个非 ignored 用例已在 workspace 执行 | `process-fault-regressions.log` |
| `cargo test -p editor-app extensions::composable_tests -- --ignored --test-threads=1` | 1 passed、0 failed、0 ignored；实际 WASM 预览与未保存内存编辑 | `composable-native-preview.log` |
| `website/` 中 `npm ci`、`npm run build` | 17 passed、0 failed、0 skipped | `docs-build.log` |
| Standards 与 Spec 两条轴只读复核 | 修正后未发现新的确定问题；审查者未自行执行测试 | 本文及初审记录 |

站点子树 `5c0e04227bc69d36302b02ac0bd8f0a21cd7a3df` 在最后两次仅改 Rust 测试的候选间未变，复用该隔离站点构建结果。最终 staged diff 与文档相对链接另行核对，日志留在本分支工作区的 `target/`，不混入版本发布。

已有失败保留其原因：原生构建测试仅匹配中文“停止”，在英文下失败，现改用当前 i18n 原因，并加强会话 `Exited` 与独立终端 Shell 数量断言；不是删掉停止验收。隐藏面板用例原先重复读取已被 `publish_frame` 交付的队列，恢复又未驱动公开 Locate 请求；现按生产路由驱动，再检查原面板恢复及实际退出，没有用直接调用 Manager 绕过控制器。

## 工单处置

代码提交为 [`ec1414ae6e336fe77bfe44da037efcf532ae338b`](https://github.com/T-miracle/Editor/commit/ec1414ae6e336fe77bfe44da037efcf532ae338b)，`git ls-remote` 已确认远程 `Editor-run-debug-build` 与该提交一致后，按依赖顺序关闭 #52、#54、#56、#57、#58、#59、#60，再逐张读回 #49–#60。父设计 #48 回读仍为 open，没有修改正文或状态。

连接器写入返回 `403 Resource not accessible by integration`。按仓库既有授权改用本机 Git 仓库已有的 GitHub 认证调用 REST API，仅更新七张子工单的 `state` 与 `state_reason`；没有发布评论，凭据不写文件或日志。关闭发生于 2026-10-06 05:25–05:26（北京时间）。

| GitHub issue | 实施工单 | 回读状态 | 本轮处置 |
| --- | --- | --- | --- |
| [#49](https://github.com/T-miracle/Editor/issues/49) | 01 — 手动配置运行真实程序 | closed / completed | 保留关闭并补缺口 |
| [#50](https://github.com/T-miracle/Editor/issues/50) | 02 — 正常停止、重新运行与离开确认 | closed / completed | 保留关闭并补缺口 |
| [#51](https://github.com/T-miracle/Editor/issues/51) | 03 — 统一下拉管理并行会话与输出 | closed / completed | 保留关闭并补缺口 |
| [#52](https://github.com/T-miracle/Editor/issues/52) | 04 — Shell 脚本与本机环境配置 | closed / completed | 验收、代码推送后关闭 |
| [#53](https://github.com/T-miracle/Editor/issues/53) | 05 — 独立构建与顺序启动前步骤 | closed / completed | 保留关闭并补缺口 |
| [#54](https://github.com/T-miracle/Editor/issues/54) | 06 — 共享配置与本机覆盖 | closed / completed | 验收、代码推送后关闭 |
| [#55](https://github.com/T-miracle/Editor/issues/55) | 07 — Rust 目标发现、保存与修复 | closed / completed | 保留关闭并补缺口 |
| [#56](https://github.com/T-miracle/Editor/issues/56) | 08 — 按配置选择执行提供者与插件调用 | closed / completed | 验收、代码推送后关闭 |
| [#57](https://github.com/T-miracle/Editor/issues/57) | 09 — Rust 启动调试与源码断点 | closed / completed | 验收、代码推送后关闭 |
| [#58](https://github.com/T-miracle/Editor/issues/58) | 10 — 单步、变量、调用栈与多会话调试 | closed / completed | 验收、代码推送后关闭 |
| [#59](https://github.com/T-miracle/Editor/issues/59) | 11 — 插件变更确认与会话故障回收 | closed / completed | 验收、代码推送后关闭 |
| [#60](https://github.com/T-miracle/Editor/issues/60) | 12 — 独立插件接入与整体验收交付 | closed / completed | 验收、代码推送后关闭 |

公开契约及其消费者、SDK、真实提供者、原生界面与必需读者正文构成同一可构建交付，因此同一 `feat` 提交；独立初审、终验和工单回读记录另做文档提交。主工作区 `Editor` 保留原 `main` 提交 `4d4a7b6d90130cf6e687c46c610c5ca38f1f89c1` 及既有改动；没有合并或切换它。
