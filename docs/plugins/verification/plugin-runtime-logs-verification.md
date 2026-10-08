# 插件运行日志验证（工单 02 / GitHub #24）

状态：已完成，2026-10-04，Windows。依据：[规格](../specs/plugin-management-logs.md)、[工单](../tickets/plugin-management-logs/02-runtime-logs.md)。底栏摘要属于 #25。

本记录保留本工单交付时的历史行为与结果。后续日期格式及日志降序要求见[本地日期与降序验证](plugin-log-time-order-verification.md)，当前显示规则以该补充和现行规格为准。

## 交付行为

- RuntimeLogs 是管理器、WASM 实例、原生语言服务和窗口共享的本次进程日志源。记录保存稳定 ID、插件 ID、宿主接收时间、Info / Warning / Error、来源和消息。每插件最多 512 条，每条最多 8192 个 Unicode 标量；淘汰最早条目和其已读状态。没有磁盘持久化，关闭窗口、切换插件或重启插件保留本次运行历史。
- WASI stdout / stderr 分别进入普通日志和警告。每个流最多缓存 32 个 4096 字节块；合法输出拥塞时丢弃超量块，后台合并警告并继续采集，不因为日志队列满使插件打印失败。缺块丢弃未完成行，避免把两段输出拼成虚假消息。非法超出 WASI 写入许可仍明确拒绝。
- LSP 进程启动、运行通知、stderr、退出、有限重试、暂停和恢复通过所属服务接入同一日志。进程故障最多等待 200 ms 收尾 stderr；主动停止先撤销发布，迟到消息不会提醒替代实例。文档诊断和终端交互输出不进入运行日志。
- 普通 LSP 通知队列最多 256 条，拥塞丢弃额外通知并每连接报告一次警告，继续采集日志。RPC 请求与响应保留可靠路径，声明式就绪状态在通知入队前独立更新；等待服务器回复沿用调用方的剩余超时预算。
- 宿主操作、服务准备、动态语法加载及真实执行故障保留插件归属。正常过期 UI revision / incarnation 继续被拒绝，记录为普通信息；真实无效交互仍记录错误。
- 日志页显示时间（明确标注 UTC）、级别、来源、所属插件和消息。警告 / 错误保留彩色图标，长正文使用主题前景色以保证可读性。Tab 图标只表示最高未读异常，错误优先于警告。
- 进入日志页只读取进入时该插件的保留记录；之后新到达条目只有在消息区域实际进入视口后才读取。关闭或切换不会让旧回调读取其他插件或较新的记录。滚动只发生在固定操作 / Tab 下方，没有筛选、清空或自动跟随。
- 记录已读与底栏提醒确认分开保存，提供按捕获边界确认及按记录读取的通用能力，供 #25 使用。

## 实际验证

在“此前提交 + 本工单 index”的导出目录验证，使用独立 Cargo target，避免其他工作树覆盖共享产物。PowerShell 设置：

```powershell
$env:CARGO_TARGET_DIR = 'C:/Projects/RustProjects/Editor/target/plugin-management-validation'
$env:RUST_MIN_STACK = '16777216'
cargo fmt --check
cargo test --workspace --exclude editor-app
cargo check --workspace
cargo build -p editor-app
cargo build -p plugin-runtime --example lsp_fixture
./scripts/build-capability-example.ps1 -HostExe C:/Projects/RustProjects/Editor/target/plugin-management-validation/debug/editor-app.exe
cargo test -p plugin-runtime --test runtime_logs -- --ignored --test-threads=1
cargo test -p editor-app extensions:: -- --test-threads=1
cargo test -p editor-app language::navigation:: -- --test-threads=1
cargo test -p editor-app language::navigation::runtime_log_tests -- --ignored --test-threads=1
cargo test -p editor-app obsolete_ui_callbacks_are_information -- --ignored --test-threads=1
cargo test -p editor-app native_restart_recovers_a_fault -- --ignored --test-threads=1
git diff --cached --check
```

| 范围 | 证据 | 最终结果 |
| --- | --- | --- |
| 来源、保留和并发 | runtime 单元回归：共享 sink、512 淘汰、Unicode 长度、唯一 ID、跨插件、捕获边界、确认与已读分离、输出拥塞及恢复 | 已通过 |
| 实际 WASM | 当前 SDK 夹具产生 stdout、panic stderr 和宿主错误；失败候选保留旧实例及日志 | 2 项显式 ignored 已通过 |
| 实际 LSP | 独立 stdio 夹具验证级别、诊断隔离、故障尾部、重试预算、恢复、退休、通知拥塞、日志就绪和堵塞回复的时限 | 9 项显式 ignored 已通过；普通 navigation 6 项通过 |
| 管理窗口 | 真实声明式 ZIP 经管理器进入 GPUI，验证跨插件、未读图标、离屏新错误、长日志、固定顶部和主题 / 字号 | 5 项通过；extensions 普通测试合计 29 项通过 |
| 退役状态 | Published 仍保留旧 plan Arc 时，禁用后迟到状态不得追加红色日志或覆盖当前状态 | 已通过 |
| 过期 UI 与恢复 | 真实 SDK 组件验证旧 revision / epoch 普通记录、真实拒绝错误及原生重启后文档不丢失 | 2 项显式 ignored 已通过 |
| Windows 原生窗口 | 独立插件根，真实服务准备失败及 LSP 运行通知，确认消息与图标 / 滚动行为 | 已通过，详见下方观察 |
| 阶段检查 | fmt、非 UI workspace 测试、workspace check、diff | 全部通过，应用已重新构建 |

原生观察：准备失败产生所属插件红色 Tab 图标；进入日志页后图标消失，条目仍保留红色错误图标、UTC 时间及实际工具路径。另一插件的普通、错误及长警告分别显示，中文 / 英文混合正文正常换行。滚轮只移动 Tab 下方内容，顶部按钮与 Tab 位置固定。重启后旧记录保留，离屏新错误再次点亮 Tab，视口没有自动跳走。关闭管理窗口后重新打开仍保留日志。浅色与深色主题均实际查看；英语界面、20 px 字号及跨插件并发读取通过 GPUI 验证，未宣称原生英语界面验收。

## 审查与边界

独立审查发现并修正离屏新记录误读、正常日志拥塞造成打印失败、崩溃 stderr 尾部丢失、退役计划迟到状态报红、浅色主题长警告正文对比度，以及文档通知拥塞阻塞日志和声明式就绪信号的问题。修正后独立复核没有剩余阻塞项。

没有升级公开协议或 SDK，没有发行插件包；capability-example 改动仅为实际 stdout 回归的测试夹具。既有 CollectionModel 可见性、未使用辅助函数和 Windows wasmtime / tree-sitter 链接警告仍存在。与本工单无关的 ignored 验收没有执行，不列为通过。
