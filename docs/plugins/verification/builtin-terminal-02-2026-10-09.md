# 内置终端第二阶段验收（#89）

范围：N04–N11、N16、N17、N26；交互调试和旧数据导入分别由 03、04 承接。固定总基线 `48e33ba1f6e8232349206b171256b66fac1fd901`，01 正式提交 `f504af857eb6fc2a6244224140269a4648d48d51`，最终只读审查候选 `ba5f39a120a837843b96ad078e24e3a7ee25aa83`。候选不移动正式分支。

## 实现与行为证据

- 配置准备、独立构建、正式运行和插件开发/打包输出进入同一个内置底部面板。任务 Tab 按配置和受理轮次归组，步骤标题、输出及成功/失败/停止结果按顺序保留；校验拒绝不清空旧记录。构建独立输出块已退役，调试检查区仍由 03 迁移。
- 内置 Shell 模板使用现有表单和校验，通过只读工具定位发现解释器；不需要终端包。语言模板与构建编排仍由相应插件负责。
- 两个真实 PTY 任务分别接收输入；普通 Shell 不被占用。任务面板打开不额外新建 Shell。已结束 Tab 复用、关闭后重建、活动关闭确认/取消、隐藏、显式重跑等待真实退出及迟到事件隔离，均由真实 GPUI 输入和进程回归覆盖。Task 的 OSC52 不可借宿主权限改写剪贴板，用户明确复制保留可用。
- 原生执行提供者通过公开 `interactive.execute` 和 `session.host` 契约协商，新增可选终端创建 2.2 与 resize 2.1；既有 2.0 无界面调用保持有效。两个不同身份的真实 WASM 消费者经公开管理器完成创建、输入、查询、订阅、定位、停止及来源撤销；无需终端包，无插件 ID 业务白名单。
- 进程结束以真实根进程、整个 Job 树退出和输出 EOF 为准，关闭受理不伪造 Terminated。真实多进程 fixture 验证最终输出及子孙进程退出；resize 失败仍观察退出，不把失败事件丢给虚构的 session 0。
- 独立关闭和强制停止使用有界的逐会话退休 gate，绕过饱和输入和输出队列。新回归先观察关闭被拒绝的 RED，再确认不排空 UI 事件也能停止第一个 PID、第二个 PID 继续存活的 GREEN。排队启动在创建前再次检查退休；失败/结束释放 gate，未知会话不留下 tombstone。
- 在独立临时工作区和开发配置根启动真实 Nanobug，无终端包：Run PackageQA 生成实际 ZIP 并显示底部任务输出；Build 使用同一个 Tab 新轮次；DevelopmentQA 创建第二个任务 Tab，启动真实开发实例，工具栏重载到 generation 1，停止后两个进程实际退出。任务状态自动变为结束，无需鼠标唤醒；重启仅恢复旧输出，不自动重放任务。
- GUI 夹具只在临时私有配置中预置空权限声明；未改用户授权或真实历史。开发 Job 的 33ms 唤醒仅绑定活动 request_id，结束或重跑后旧 watcher 收敛。

## 检查

Windows / Rust 1.95 / 已安装 MSVC，保持当前 GPUI 依赖。测试设置 `RUST_MIN_STACK=16777216`；本机进程级 `LIB` 指向已安装 MSVC 14.50 与 Windows SDK 10.0.26100，不修改全局环境或安装工具。

| 命令 | 结果 |
| --- | --- |
| `cargo test -p plugin-runtime --lib native_processes::tests -- --test-threads=1` | 4 通过，包含真实单会话饱和退休、最终输出/树退出、权限撤销及重新授权 |
| `cargo test -p editor-app builtin_task -- --test-threads=1` | 3 通过，真实 PTY、步骤序列、关闭/复用/重新运行、同帧输出归组与剪贴板边界 |
| `cargo test -p editor-app terminal:: -- --test-threads=1` | 本单前候选 13 通过、1 ignored；最后改动仅涉及监督器退休和任务步骤结果，相关 3 项另行最终回归 |
| `cargo test -p plugin-runtime --test interactive_execution native_execution_two_consumers -- --ignored --test-threads=1` | 显式实际双 WASM 消费者 1 通过；消费者从当前内嵌 SDK 构建 |
| `npm test`（website） | SDK 双语文档检查 17 通过 |
| `cargo fmt --check` | 通过 |
| `cargo test --workspace --exclude editor-app` | 所有非 ignored 用例通过；实际包 ignored 未运行的部分不计通过 |
| `cargo check --workspace` | 通过 |
| `cargo build -p editor-app` | 通过；用于真实 Windows GUI 的原生调试产物 |

最终阶段门禁日志为临时目录 `nanobug-t02-*-delivery.log`，退休 RED/GREEN 为 `nanobug-t02-priority-{red,green}.log`。实际公开消费者使用 `target/plugin-api-test/capability-example.zip`（0.17.0）。01 的主题、中文/emoji、Neovim 和共享 Tab/滚动控件证据仅在未改变对应输入/绘制/夹具的范围复用，不将其重新计数。macOS/Linux 未在本 Windows 环境运行；Windows 实体 IME 候选窗口未另做验收。既有 unused/private-interface 警告保留。

## Standards

通过 [code-review](C:/Users/Tmiracle/.agents/skills/code-review/SKILL.md) 的规范轴只读审查。发现并修复队列饱和阻塞单来源退休、原生句柄提前分离、Task OSC52 越权、开发 Job 无自动唤醒/旧 watcher 跟随新轮次、提供者本地身份碰撞及 Shell 文本未本地化。最后候选复审：硬性问题 0，判断项 0。

## Spec

规格轴发现并修复步骤尾输出晚于下一标题、静默失败结果未保留、真实树退出未等待、原生故障未归属、Task 容量拒绝后已启动进程没有回收等问题。针对受影响行为补实际回归，未新增调试输入或迁移范围。最后候选复审无遗留问题。两轴审查完成后才提交、推送并读回 #89 状态；父设计议题 #87 保持不变。
