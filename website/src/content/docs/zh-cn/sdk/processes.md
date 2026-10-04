---
title: 原生进程
description: 声明式服务、授权执行、传输方式、进程归属与资源上限。
section: sdk
order: 3
alternate: /en/sdk/processes/
---

# 原生进程

独立协商 `process: ^1`。通过普通 SDK `request` 发送
`api::Operation::Process { operation }`；能力不由插件名称决定。

## 声明与授权

```json
{
  "services": { "analysis": { "program": "analysis-server", "args": ["--stdio"] } },
  "permissions": ["process.service.analysis"]
}
```

`StartService { service: "analysis" }` 只能传声明 ID，不能覆盖程序、追加参数、
修改环境、工作目录或插入安装步骤。每项服务独立授权，安装弹窗展示程序及参数数组。
更新新增权限需确认；拒绝后旧包、旧授权及运行实例保持可用。

`Execute { program, args, transport, cwd }` 单独要求 **`process.exec`**，允许显式选择
程序（包括解释器），不能由普通服务权限获得。当前没有原生安装器操作。

`cwd` 默认为空；显式指定需要协商 `process >=1.2`，且必须是存在的绝对目录，长度不超过 4096 字节、不含 NUL。它只选择已授权任意进程的启动目录，不授予 WASM 文件访问权限，也不能用于覆盖 `StartService` 的声明。省略时继续使用实例默认目录。

通用 toolchains 解析器只接受绝对可执行路径，或在 PATH 的绝对目录中查找裸工具名；
不隐式搜索当前项目，不拼接 Shell 命令。Windows 只启动 `.exe`。工作区实例使用其
受信任项目作为 cwd；应用实例和无项目实例使用自己的私有数据目录。

原生程序以当前用户权限访问文件和网络，WASM 沙箱不约束其内部行为。撤销工作区信任
会停止工作区实例和进程树。项目配置不能授予上述权限。

## 传输、结果和资源归属

- 声明服务固定使用 **Stdio**，stdout/stderr 是独立字节流，适合 LSP/JSON-RPC，
  不引入终端转义和行转换。
- 显式执行可以选择 Stdio 或 **Pty { columns, rows, inherit_cursor }**。PTY 合并输出，可能包含
  终端转义；宿主不解释终端画面。
- `inherit_cursor` 默认 `false`，序列化时省略默认值，保留旧能力消费者的行为。设为 `true` 需要 `process >=1.3`，且仅 Windows 支持；其他平台在创建进程前返回 `UnsupportedOperation`。Windows 使用标准 `PSEUDOCONSOLE_INHERIT_CURSOR`，将光标查询原样交给字节流消费者，由插件通过 `Write` 异步回答。宿主不解析提示符、快照或光标回复；不会根据插件 ID 自动启用。协议依据见 [CreatePseudoConsole](https://learn.microsoft.com/en-us/windows/console/createpseudoconsole)。
- 启动返回 `Value::Resource`，句柄绑定实例和作用域。其他插件、已销毁实例及进程
  退出后的旧句柄无法使用它。
- `Write` 返回 `Unit` 表示进入有界输入队列，不代表程序已消费或执行这些输入。
- `Resize` 仅用于 PTY，限制尺寸并在 150 ms 内合并连续调整；标准管道不能调整尺寸。
  Windows 原生调整在独立工作线程执行，最多保留一个待执行的最新尺寸；返回表示接受请求。插件不回复光标查询也不能阻塞宿主的输入、轮询和终止；后续轮询报告原生调整错误。退役或启动失败时，宿主先异步终止并确认整个 Job 内进程全部退出，才向已请求握手的 OS 管道发送固定中性光标回复以解除等待；该清理回复不会交给存活程序，也不读取插件的终端状态。待原生调整结束后唯一关闭控制台，回收所属管道和 Job。
- `Notification::Process { handle, update }` 提供带流类别的输出和数字退出码。
  读到 EOF 并排空输出后才发送 Exited，不因暂时空队列丢掉退出前的大段输出。
- `Terminate` 和 `CloseResource` 释放句柄并向 OS 发出终止进程树请求，分别返回
  `Value::Process(Update::Terminated)` 与通用 `Unit`。该结果表示资源关闭、终止已发出，不表示撤销
  程序对文件或网络的修改；插件主动关闭后的句柄不再收到通知。
- 服务调用来源退出时，宿主回收委托进程，并向仍活动的提供者发送一次 `Terminated` 清理通知。
  此通知保留失效的来源权限，允许更新本地界面，不允许借回调启动新操作或读取私有数据。
  提供者本身也退出时不会回调；详细来源规则见 `SERVICES.md`。
- 停用、卸载、替换或销毁拥有者也会清理资源。Windows 使用 kill-on-close Job：
  PTY 在创建时原子加入 Job；管道进程暂停启动，加入 Job 后才恢复，避免提前派生后代。
  macOS/Linux 保持编译目标；本轮进程树保证及原生验收针对 Windows。

每实例最多 32 个进程、128 个总资源句柄。单次输入至多 1 MiB，每进程最多排队 8 次；
输出块为 8 KiB，队列容量 64，以反压控制积压，每轮有界读取。权限不足、能力未协商、
失效句柄、实例未激活、请求超限和原生 I/O 失败通过统一类型化错误返回。
