# 工单 20：统一能力协议与完整契约验收

对应 [GitHub #21](https://github.com/T-miracle/Editor/issues/21)，规格为 [插件平台方案](plugin-api-platform.md)。审查固定点 `147ba8153a57de164d0db7eee4f3caef0027a339`，包含工作区新增文件。执行环境：2026-10-03，Windows。

## 交付边界

公共 SDK 0.2.0 删除旧 `Message/Event/Reply/Request`、`Scene/Widget/CanvasControls`。宿主直接处理 `api::Input/Output/Notification` 和组合 `ui::Document`，不再把新界面转换成旧场景。`Manager::event` 显式携带面板身份。请求关联、权限、实例所有权、UI revision、文档 revision、工作线程 epoch 与失效资源清理继续生效。

语言识别与高亮使用独立声明；LSP 只能使用经过运行时校验的服务计划，不能走旧 `lsp_command`、直接原生进程启动或旧 readiness 分支。旧合并式 `languages` 声明明确拒绝。主题文件可在导入时规范化历史命名空间，但运行时不再向新插件输出旧 `me.*` 别名。

Rust SDK 的旧类型移除是源码破坏性变更，因此提升 SDK 次版本并重建消费者；线上能力协议仍为清单 protocol 7、base 1，WIT 世界保持 0.1.0。能力分别协商版本。旧协议包在安装及恢复前拒绝，设置、作用域、快照和私有文件通过既有有限数据导入保留。

最终实际终端检查还发现 `process 1.2` 默认 PTY 启动会覆盖已恢复画面。补充通用 `process 1.3` 的可选 `inherit_cursor`，默认关闭且不序列化默认值；Windows 插件按需响应公开字节流中的光标查询。宿主没有终端插件识别或提示符解析。Windows 尺寸调整使用有界后台队列；退役/失败后先确认整个 Job 已退出，再以固定中性回复解除 OS 握手。详见 [公开进程协议](../../crates/plugin-protocol/PROCESSES.md)。

交付包为 terminal 0.7.1、example 0.3.1、svg 0.2.1、rust/toml/html/javascript 0.2.1。capability-example 0.15.1 仅供验收，不加入发行插件目录。语言资源清单与包版本一致，所有实际 ZIP 包含 README。声明式资源包继续不携带空 WASM。

## 验证命令与结果

日志位于忽略目录 `target/plugin-api-publication/contract-*.log`。真实包验收在隔离目录执行；SDK 独立构建在仓库外临时目录执行，不操作用户实际插件安装数据。以下批次均已执行通过，重复检查不计为新增测试。

| 验证批次 | 实际结果 |
| --- | --- |
| 最终非 UI workspace 测试 | 54 通过、0 失败；52 项默认忽略由下述实际包检查覆盖 |
| 原生编辑器常规测试 | 169 通过、0 失败；24 项默认忽略单独显式执行 |
| 运行时真实包全套 | 51 通过、0 失败、0 忽略，2148.98 秒 |
| 原生真实包及 Rust Analyzer | 24 通过、0 失败、0 忽略，756.23 秒 |
| PTY 修正后的受影响真实包复验 | 进程 5 项、执行服务 3 项、终端迁移 1 项全部通过 |
| 固定基线的最终回收复验 | 新增光标握手及回收测试通过，29.94 秒；覆盖第 52 个运行时真实包用例 |
| 原生终端服务面板复验 | 1 项通过，27.31 秒 |
| 独立插件单元测试 | terminal 43、example 1、svg 5、rust 2，全部通过 |
| 实际终端启动与缩放 | 两种启动时序、四组恢复尺寸、慢速及快速连续拖拽、实际命令输出均通过 |
| UI 与 SVG | 示例交互、SVG 缩放/编辑恢复/热更新 smoke、原生透明栅格渲染均通过；已查看生成图像 |
| 七个交付 ZIP | 分别安装、checkpoint、停用、重启实例、卸载全部通过；版本和 README 已核对 |
| SDK 与编译 | 仓库外独立构建、导出逐文件修复、根工作区及五个独立插件格式、workspace 编译检查全部通过 |

- 工作区：`cargo fmt --check`、`cargo test --workspace --exclude editor-app`、`cargo check --workspace`。
- 原生界面：`RUST_MIN_STACK=16777216`，`cargo test -p editor-app --bin editor-app -- --test-threads=1`。测试使用串行执行，避免全局语言注册表相互干扰。
- 夹具及实际包：`cargo build -p editor-app`、`cargo build -p plugin-runtime --examples`、`./scripts/build-plugins.ps1 -HostExe ./target/debug/editor-app.exe`、`./scripts/build-capability-example.ps1`、`./scripts/verify-plugin-sdk.ps1`。
- 真实契约：`cargo test -p plugin-runtime --tests --no-fail-fast -- --ignored --test-threads=1`；编辑器先以 `cargo test -p editor-app --bin editor-app --no-run --message-format=json` 取得测试可执行文件，再直接运行 `--ignored --test-threads=1`，避免实际 Rust Analyzer 等待父 Cargo 的 target 锁。
- 终端尺寸：`cargo run -p plugin-runtime --example terminal_startup -- dist/plugins/terminal.zip` 和 `terminal_resize`，通过实际 ConPTY、PowerShell、磁盘快照及组合 Canvas 检查提示符、命令输出、行距、宽高反复变化和恢复。
- 追加回归：`cargo test -p plugin-runtime --test process_capabilities --test terminal_migration --test interactive_execution -- --ignored --test-threads=1`；固定基线复验使用 `--test process_capabilities cursor_handshake`；原生终端面板使用 `cargo test -p editor-app --bin editor-app execution_service_tests -- --ignored --test-threads=1`。
- 插件单元：以根 `target` 为 `CARGO_TARGET_DIR`，分别执行宿主 `--plugin-cargo plugins/<名称>/Cargo.toml test --lib`。包入口还执行 `ui_smoke`、`svg_preview_smoke`、七包 `smoke`，以及 `svg_preview_render` 的真实 PNG 检查。

默认忽略项与显式执行项分别记录，不把跳过视为通过。Windows 既有链接器和未使用代码警告不掩盖错误。其他操作系统未在本轮执行原生行为或交叉构建，不宣称已验证。

## T01–T26 对应证据

运行时测试目录为 `crates/plugin-runtime/tests/`；原生集成目录为 `crates/editor-app/src/extensions/`。下表列出具体行为入口，最终运行结果见上一节。

| 验收 | 当前验证入口与观察结果 |
| --- | --- |
| T01 | `language_tests`：陌生声明包安装到已打开文档，高亮出现，卸载撤销 |
| T02 | `lsp_tests`、`dependency_tests`：独立原生服务收到动态启动配置与初始化字段 |
| T03 | `language_tests`：多语言贡献、损坏 grammar 隔离、迟到加载不能复活 |
| T04 | `language_tests`、`service_tests`、`plugin_services`：提供者替换及项目选择保持稳定 |
| T05 | 提供者选择测试覆盖单一候选、多候选和无候选，不按安装顺序覆盖 |
| T06 | `managed_dependencies`、`native_installers`：私有缓存、校验、离线复用、活跃租约保护 |
| T07 | 同上及 `dependency_tests`：下载/校验/安装失败、取消及拒绝不会发布半成品 |
| T08 | `hot_update`、`hot_update_tests`、`ui_package_tests`：界面与未保存文档随热生命周期变化 |
| T09 | `data_migration`、`hot_update`：迁移、激活、提交失败及中断恢复 |
| T10 | `hot_update`、`worker_tests`：候选准备期间旧实例写入，切换取得最终数据 |
| T11 | `editor_requests`、`preview_tests`、`lsp_tests`：过期 revision、实例和租约结果被拒绝 |
| T12 | `editor_requests`、LSP `service::tests`：受理与完成分离、超时取消、关闭后无迟到结果 |
| T13 | `editor_requests`、`process_capabilities`：有界文档事件、进程流量与关闭后的批次回收 |
| T14 | `capability_packages`：必需能力拒绝、可选能力降级、错误与请求 ID 关联 |
| T15 | `composable_ui`、`composable_tests`、`ui::plugin::tests`：表单、Canvas、组合树、焦点与 IME |
| T16 | `scoped_instances`：两个工作区私有文件、句柄、配置与事件隔离 |
| T17 | 同上：应用级实例跨工作区关闭存活，不能借用工作区权限 |
| T18 | `process_capabilities`、`native_installers`：新增权限与具体安装授权，拒绝后旧实例保留 |
| T19 | `process_capabilities`：声明服务不能改为任意程序，受限工作区不执行 |
| T20 | `plugin_services`、`interactive_execution`：替代提供者、来源权限、退出、失效引用及不重放 |
| T21 | `fault_recovery`、`recovery_tests`：WASM 预算、LSP 有限重试、手动恢复、文档继续编辑 |
| T22 | 上述生命周期测试、`native_ui_tests`、`execution_service_tests`：进程、任务和面板占位回收 |
| T23 | `settings`、`settings_tests`、`sdk_discovery`：配置层级、生效来源、显式错误不回退 |
| T24 | `compatibility`、`installed_migration`、`data_migration`：旧包拒绝、范围与数据保留、有限导入 |
| T25 | `terminal_migration`、终端两个尺寸例、`dock_tests`、`ui_migrations`、`ui_package_tests`、实际 Rust Analyzer 测试 |
| T26 | SDK 仓库外构建与逐文件修复校验；静态检查旧执行入口、插件 ID/语言名分支、源码依赖与 vendor |

光标握手补充验收使用独立 capability-example 包，验证任意消费者能够收取查询并回复、故意不回复时的尺寸请求、终止、直接停用、外部结束及反复启动失败。检查 OS 句柄回落，不能只以运行时进程计数为零判定资源已释放。首次原生错误路径存在固定初始化开销；跟踪确认 ClosePseudoConsole 已返回后，测试先预热该路径，再检查重复创建没有持续增长。

## 收缩与审查

旧通道专用测试由现有公开包权限、作用域、请求、热更新、原生输入和真实插件检查承接。通用 smoke 只负责包生命周期，不再描述为完整 ConPTY 验收。保留并迁移真实终端启动/缩放例，新增原生 Dock 拖拽、尺寸持久化、隐藏回收及重开恢复回归。删除重复包含内部语言模块的旧 language_diagnostics 示例壳；保存/未保存语义诊断测试保留在主程序测试目标，并在本轮显式执行。

### Standards

独立规范审查与最后收缩部分追加复核：硬性违规 0 项，判断性坏味道 0 项。LSP 受控计划、公开包测试边界、注释与数据导入规则符合当前仓库约定。

### Spec

初审发现尺寸断言与旧文档入口遗漏，已恢复相应检查并修正文档；后续发现的旧 selector 和绘制稳定性判断也已迁移到当前接口。最终独立静态复核 0 项遗留；原生 Dock 针对性测试已通过，真实尺寸例结果见验证记录。

最终补充审查要求使用固定句柄基线，并将异步清理错误接入实际诊断接收端。现由 Manager 持有有界诊断队列，退役后的报告仍保留插件、作用域、进程和操作归属；WASM 与原生错误共用递增序号，统一合并后保留最新 32 条，避免旧清理错误遮蔽新故障。相应混合来源与异步归属回归通过，两条审查轴均为 0 项遗留。

能力拒绝测试原先只检查错误链外层字符串；新调度保留插件/作用域上下文后，断言改为读取结构化 Failure.code。权限拒绝行为保持正确，真实包两项测试通过。

## 使用限制

授权的依赖准备仍需要安装权限；额外安装步骤及 SDK 选择使用已有具体确认流程。热更新恢复逻辑状态及受管私有数据，不恢复进程内存或回滚外部系统副作用。原生程序具有操作系统用户权限，私有安装目录不等于操作系统沙箱。没有新增后台自动升级、原生插件模块或专属宿主接口。
