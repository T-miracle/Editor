# 运行会话 — interactive.execute 2.0 与 session.host 2.0

[English](../../en/sdk/sessions.md)

Nanobug 内置 `interactive.execute` 2.2 原生提供者，无需安装终端包。没有用户或工作区显式
选择时使用内置提供者；兼容的显式选择仍优先，且不重定向已创建的会话。插件通过
`session.host` 在统一终端面板创建可见任务 Tab 及归属自己的订阅。直接调用提供者不会
创建宿主会话记录。

2.1 额外提供 `resize(session, columns, rows)`，需要 `process.exec` 权限。列数为 2–1000，
行数为 1–500，结果为 session、state。消费者只在使用尺寸接口时声明
`execution::resize_method()`；2.0 的六个必需方法保持精确结构，所以已有 2.0 消费者仍兼容。
原生执行最多保留 64 条会话记录，只淘汰已结束历史。

2.2 还提供 `execute_terminal`，参数为 execute 的字段加必需的 inherit_cursor 布尔值，
创建结果相同。使用时声明 `execution::terminal_execute_method()`。Windows VT 界面传 true，
通过 input 回答 ConPTY 的初始光标查询，让后续准备步骤接在旧输出之后。无界面消费者使用
execute，Unix 界面传 false；资源权限与归属规则相同。

运行服务建立在[插件服务](services.md)上。提供者发布精确的方法结构、权限与版本；消费者声明所需方法。宿主按兼容契约与逻辑作用域选择提供者，提供者名称不产生授权。

## 交互执行

公开 SDK 的 `execution::observation_methods()` 提供 input、locate 和 events 的标准声明；
`execution::resize_method()` 和 `execution::terminal_execute_method()` 声明可选扩展。

| 方法 | 参数 | 结果 | 权限 |
| --- | --- | --- | --- |
| execute | program、args；可选 cwd、name、env | session、state | process.exec、ui.panels |
| stop | session；可选 mode | session、state | process.exec |
| status | session | session、state；可选 code | process.exec |
| input | session、bytes | session、state | process.exec |
| locate | session | session、state | ui.panels |
| events | session、after、limit | session、events、cursor、gap | process.exec |

program 是可执行程序，args 是字面参数数组，最多 128 项，每项 4096 UTF-8 字节；宿主不拼接 Shell 命令。环境覆盖最多 64 项，名称 128 字节，值 32768 字节；值只传给子进程，不记录。输入最多 1024 个 0–255 整数字节。

请求进入队列、提供者创建进程、观察到进程退出是不同状态。未知提供者状态报告错误。正常退出可以报告完整的 32 位无符号原生退出码；缺省退出码不代表零。强制终止报告 terminated，不使用退出码哨兵。

提供者对每次会话操作校验宿主认证的调用实例。locate 恢复同一会话的提供者展示；隐藏受管视图保留程序、输入与输出。提供者自行创建的普通 Shell 关闭行为可以不同。

## 增量事件

输出事件包含 sequence、kind=output、stream（stdout、stderr 或 pty）及最多 512 个原始字节。状态事件包含 sequence、kind=state、state 和可选 code。消费者跨片段解码 UTF-8 与 ANSI。

`execution::EventBuffer` 最多保留 128 项事件。每次读取游标后的 1–16 项；返回游标为最后送达的序号，空批次不前进。gap=true 明确表示历史被淘汰；未来游标和乱序事件不能伪装成完整输出。原生提供者允许 32 个活动进程，最多保留 64 个会话记录，只回收完成历史，不挤出活动程序；已回收的输出或展示返回 InvalidHandle。

## 宿主统一会话

SDK 的 `execution::host_dependency()` 声明 session.host ^2。创建的记录与编辑器运行控件共用。

| 方法 | 参数 | 结果 | 来源权限 |
| --- | --- | --- | --- |
| start | 交互执行字段；可选 configuration | session、state、located | process.exec、ui.panels |
| list | 空记录 | sessions，每项含 session、state | 无额外权限 |
| status | session | session、state | 无额外权限 |
| stop | session；可选 mode、grace_ms | session、state | process.exec |
| input | session、bytes | session、state | process.exec |
| locate | session | session、state | ui.panels |
| subscribe | session | subscription、session、state | 无额外权限 |
| next | subscription、limit（1–16） | subscription、session、state、events、cursor、gap | 无额外权限 |
| unsubscribe | subscription | subscription | 无额外权限 |

所有消费者仍须协商 plugin.services 并拥有 services.call。只有原工作区的原始存活实例可以访问其记录与订阅；其他插件不能列举、读取、停止或借用。编辑器窗口可管理所属工作区全部会话；委派保留原始权限与存活链。

状态为 starting、running、stopping、terminating、exited、failed。默认 graceful 停止，宽限 3000 ms；grace_ms 可为 1–60000。不支持正常退出或到期时升级为 force；显式 force 立即作用于所属进程树。停止受理不等于最终退出，必须观察实际原生结束；无法确认强制清理时报告失败，不报告成功。已完成会话重复停止幂等。提供者状态查询返回 InvalidHandle 时记录失败并封闭；临时错误不能解释为成功退出。

可选 configuration 身份为 1–256 个可打印字节。同一来源和工作区的活动同配置会被定位，即使命令已经编辑；命令相同的不同配置仍独立。无配置身份时按字面命令去重。最多保留 64 条，只淘汰已完成或失败历史；提供者选择变化不重路由已有会话。

订阅拉取提供者保留历史，每来源最多 32 个、宿主最多 128 个，每订阅只允许一个待完成拉取。next 等真实提供者回复才完成；取消拉取不前进游标。取消订阅释放等待和游标，不停止程序。提供者或历史退役、来源退役、工作区切换和窗口关闭撤销订阅；旧身份不会重新获得授权。

2.0 要求公开输入、定位和观察、显式来源权限、正常／强制停止与无符号退出码。旧 ^1 消费者和提供者须更新，不存在静默省略这些要求的兼容路径。
