# 运行与调试服务契约 `interactive.execute` 1.3 / `debug.session` 1.0

本文件说明插件如何提供「运行程序」与「调试程序」两类能力，以及消费者如何经公开服务调用它们。
两套契约都建立在 [SERVICES.md](SERVICES.md) 的服务机制之上：消费者声明契约 ID 与 SemVer 范围，
提供者用 `plugin_services.provides` 发布，宿主按契约与逻辑作用域选择提供者，
**从不按插件 ID、语言名或调试器名分支**。

宿主是这两套契约的消费者。宿主把自己的要求写成声明，提供者**只有逐形状声明**
（参数、结果、权限、版本范围均相同）才算匹配；因此「看起来可用但永远不返回」的提供者属于
**不兼容**，而不是被当作可用。允许提供者额外声明其他方法，但不允许改变宿主调用的方法形状。

## `interactive.execute` 1.3 —— 启动、观察与停止程序

宿主要求的三个方法（缺一即不兼容）：

| 方法 | 参数 | 结果 | 权限 |
| --- | --- | --- | --- |
| `execute` | `program`、`args`、可选 `cwd`、`name`、`env` | `session`、`state` | `process.exec`、`ui.panels` |
| `stop` | `session` | `session`、`state` | `process.exec` |
| `status` | `session` | `session`、`state`、可选 `code` | `process.exec` |

- `program` 与 `args` 是**可执行路径与参数数组**，宿主从不拼接 Shell 字符串；
  `args` 最多 128 项、每项 4096 字节。带空格或引号的参数保持为**单个参数**。
- `cwd` 是程序的工作目录；`name` 是显示用标签，提供者可自行呈现。
- `env` 是调用方的环境覆盖，最多 64 项，名为 128 字节、**值为 32768 字节**；
  提供者把它转交给程序，**不读取也不记录**其中的值。宿主不提供、提供者不推断未请求的变量。
- `state` 由提供者的字词决定；宿主对自己不认识的字词**报错而不是映射**成已知状态。
- `status.code` 只有提供者**观测到**程序退出时才出现（正常退出为退出码，被强制终止按提供者的
  约定值）。省略 `code` 表示「还不知道」，**不是**「退出码为零」。
- **请求被接受不等于进程已创建，也不等于已结束。** 这三个是不同的观察：
  请求排队、提供者确认创建、提供者报告结束。宿主不会用经过的时间或输出内容替代其中任何一个。

能力版本：宿主接受 `>=1.3, <2`。`1.3` 引入 `status` 与 `env`；缺少 `status` 的提供者不兼容，
因为宿主承诺的「步骤完成 = 观察到程序结束」需要它。

## `debug.session` 1.0 —— 启动调试、断点、暂停与检查

必需方法（缺一即不兼容）：`start`、`set_breakpoints`、`resume`、`pause`、`status`、`stop`。
可选能力：`step`（单步）、`frames` 与 `variables`（检查，**两者须同时声明**才算提供）。

| 方法 | 参数 | 结果 | 备注 |
| --- | --- | --- | --- |
| `start` | `program`、`args`、可选 `cwd`、`name`、`env`、`breakpoints`、`stop_on_entry` | `session`、`state` | 权限含 `ui.panels` |
| `set_breakpoints` | `session`、`breakpoints`（`source`、`line`） | `breakpoints`（含 `verified`） | 每源一组 |
| `resume` / `pause` | `session` | `session`、`state` | |
| `step` | `session`、`kind`（`into` / `over` / `out`） | `session`、`state` | 可选能力 |
| `frames` | `session` | `frames`（`id`、`name`、`source`、`line`） | 可选能力，最多 256 帧 |
| `variables` | `session`、`frame` | `variables`（`name`、`value`） | 可选能力，每帧最多 512 项 |
| `status` | `session` | `session`、`state`、可选 `reason`、`source`、`line` | |
| `stop` | `session` | `session`、`state` | 结束会话及其目标 |

- **单步与检查是可选能力**，因为它们只是能力而不是会话的前提：不会单步的提供者仍然是调试
  提供者，缺失的能力由需要它的控件**带原因禁用**，而不是让提供者整体不可用。
- `set_breakpoints`、`resume` 与 `pause` 是必需的：宿主的面板承诺设置断点与暂停/继续，
  做不到这些的提供者**在按键时才会失败**，所以按不兼容处理。
- **未验证的断点不算已设置**：`verified: false`（或省略）表示提供者无法在目标中绑定该位置。
- `frames` 与 `variables` 报告的是**已停止目标**的信息，不是程序打印的文本；
  两者都不是宿主可以自行推导或伪造的内容。
- 调试只按本契约选择提供者；适配协议、传输方式与具体调试器都**属于提供者的内部实现**，
  宿主既不声明也不假设（见 SERVICES.md 的调试前置决定）。

## 消费者如何调用

消费者（含宿主）用 `service::guest::open(contract)` 取得引用，再用
`service::guest::Task::start(&reference, method, arguments, timeout_ms)` 发起请求，
结果经 `Notification::Service(Request { handle, update })` 投递。引用与任务用
`api::guest::close_resource` 释放；提供者切换、退出或重建都会使旧引用失效，
**切回原提供者也不会复活旧引用**，宿主也不会借恢复重新执行用户命令。

## 生命周期与清理

- **提供者拥有它启动的东西。** 会话随提供者实例存在：提供者被停用、卸载、更新或崩溃后，
  相关会话明确失败，其目标进程、调试器、订阅与展示由清理路径回收，**不影响无关会话**。
- 关闭窗口时宿主先通过启动会话的提供者请求停止，再退休会话与拆除实例；每个应答有界等待，
  不可响应的提供者只耽误片刻。
- 旧提供者的引用、停止通知或迟到事件**不会复活会话**，也不能对新实例发起操作。
- 停用、卸载或更新插件前，宿主列出受影响会话；**取消则保持会话与插件原样**。

## 示例与验证

公开 SDK 的示例访客是 `plugins/capability-example`；`scripts/verify-plugin-sdk.ps1` 用宿主导出的
SDK 在仓库之外独立构建并打包它，证明接入不需要仓库内部 crate，也不需要 SDK 源码副本。
执行与调试契约的真实包验收位于 `crates/plugin-runtime/tests/host_execution.rs` 与
`interactive_execution.rs`。
