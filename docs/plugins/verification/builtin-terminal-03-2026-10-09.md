# 内置终端第三阶段验收（#90）

范围：N03、N12–N15，以及调试涉及的 02 生命周期交集。累计审查基线 `48e33ba1f6e8232349206b171256b66fac1fd901`；02 提交 `53721590e8b4f33980bc1f55de6e4bd32f3ec8a2`；最终只读代码候选 `4143eb686108126e41a3bd130086a602fa4d492b`。04 旧用户迁移尚未完成。

## 行为与契约

- Shell、准备、运行与调试使用同一个底部终端；调试检查 Entity 位于选中任务的命令列内部，侧栏仍覆盖完整面板高度。检查区使用 RunControls 的会话及暂停代次，不维护第二套调试状态；延迟操作重新认证原会话。
- Rust 调试器 0.2.0 使用 CodeLLDB 的集成终端反向请求，通过公开 `process` 1.7 将原有受管 PTY 展示给宿主。目标仅由调试器启动一次；键盘输入进入目标 PTY，DAP 标准输入输出保持独立。`runInTerminal` 的启动代理保留参数并去掉清屏请求，准备输出继续保留。
- `PresentTerminal` 只展示已经拥有的 PTY。`TerminalOutput` 仅发布已解码、每次至多 16 KiB 的只读诊断；adapter stdout 不作为原始 DAP 字节进入终端。展示模式不能改变，只读 PTY 也拒绝宿主输入和 resize。
- 原始配置、受理轮次和来源作用域在创建时冻结；全局实例创建的展示进程仍属于当时的工作区。关闭、禁用、切换工作区或撤销信任均退休相关资源，保留实际退出观察，不串入其他工作区。
- 准备结束到目标创建之间的启动间隙不会把新的目标标成已退出。启动中确认关闭等待原会话身份后强制清理；失败不会伪造关闭成功或另起普通运行副本。独立正常停止在 3000 ms 后升级 Force，stdio 不支持普通中断则直接 Force。
- 错误不截断同批其他进程的最终输出；Exit/Terminated 都清理委托上下文。调试任务的持久数据只保留输出和最后意图，不保存活句柄，也不自动重放目标。

## 检查与产物

Windows / Rust 1.95 / 已安装 MSVC 14.50 与 SDK 10.0.26100。`RUST_MIN_STACK=16777216`；仅为测试进程设置 `LIB` / `LINK`。首次全量链接缺少 `msvcrt.lib`，显式指定已安装库目录后重跑通过，未安装或改变系统工具链。

| 命令或操作 | 实际结果 |
| --- | --- |
| `cargo test -p plugin-runtime --test real_debugging -- --ignored --test-threads=1` | 7 通过，105.72 s；真实断点/暂停/继续/三种步进、多会话、stdin、失败进程树清理、拒绝能力、解码诊断与 stderr |
| `cargo test -p editor-app native_debug_stdin_reaches -- --ignored --test-threads=1` | 原生 GPUI 输入 `7`、Enter，唯一目标 PID、断点变量 `7`、结果 `15`；检查区在该 Tab 内，见最终 `nanobug-t03-final-native.log` |
| `cargo test -p editor-app native_rust_debug_controls -- --ignored --test-threads=1` | 1 通过，31.87 s；原生断点、步入实际函数、变量、取消关闭与停止目标树 |
| `cargo test -p plugin-runtime --test terminal_presentation -- --ignored --nocapture` | 1 通过，60.58 s；两种实例作用域、实际 PTY 输入、外来/过期句柄、只读模式、容量、忽略中断的正常停止、切换工作区、撤销信任后原 PID 退出 |
| `cargo test -p editor-app close_before_debug_creation -- --test-threads=1` | 1 通过；原启动身份与下一轮不会混用 |
| `cargo test -p editor-app run:: -- --test-threads=1` | 105 通过、11 ignored；未运行的真实包用例不计通过 |
| `cargo fmt --check`、`cargo check --workspace` | 通过；最终补测后再次检查 |
| `cargo test --workspace --exclude editor-app -j 1` | 非 ignored 用例全通过；最后信任清理的小增量另外由实际公开包/PID 测试覆盖 |
| `npm test`（website） | 17 通过；对应 SDK 双语文档 |

实际消费者由当前宿主内嵌 SDK 构建：`dist/plugins/rust-debugger-0.2.0.zip`、`target/plugin-api-test/capability-example-0.17.1.zip`。使用已验证的官方 CodeLLDB 1.12.3 Windows VSIX（SHA-256 `a916e509308dac817732f63ca604a8b93ed29cd16f38a2fa9f0b64ed58e8f51a`），没有终端包参与新调试验收。stdin 用例先通过旧包观察超时 RED，再重建当前消费者观察 GREEN。

新增作用域夹具最初误用现行服务不允许的跨作用域调用，已改为直接 application 消费者；该夹具失败不算产品回归 RED。最终包用例验证无 host task owner 的真实全局 PTY。临时日志前缀为 `nanobug-t03-*`；全量的第一轮链接失败记录保留，不报告为通过。

本阶段 GPUI 验收使用真实 Windows 原生测试窗口、Canvas 命中和目标进程，未将其描述为人工 GUI 操作；01/02 人工主题、TUI、开发构建等未受影响证据按范围复用。macOS/Linux 和实体 Windows IME 候选窗口本阶段未验收。现有 unused/private-interface 警告保留。

## Standards

[code-review](C:/Users/Tmiracle/.agents/skills/code-review/SKILL.md) 规范轴发现并修复跨作用域展示、只读 PTY 输入、错误导致批量事件丢失、正常停止缺少升级以及信任撤销跳过全局进程退休。最终候选硬性问题 0，阻塞性坏味道 0。

## Spec

规格轴发现并修复调试诊断没有归组、启动间隙拒绝输入、启动中关闭没有等待目标身份及只读模式改变。最终候选没有剩余确定缺陷或范围蔓延。完成验证后提交、推送并读回 #90；父议题 #87 保持不变。
