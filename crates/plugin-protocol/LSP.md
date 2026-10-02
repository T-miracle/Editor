# LSP 能力 1.0

清单使用 `protocol: 7`，必需能力为 `language.lsp: ^1` 和 `process: ^1`。
`language_servers` 与识别、高亮贡献独立；纯声明包不需要空生命周期 WASM。

```json
{
  "services": {"analysis": {"program": "analysis-server", "args": ["--stdio"]}},
  "permissions": ["process.service.analysis"],
  "language_servers": [{
    "id": "analysis", "language": "novel", "service": "analysis",
    "initialization_options": {"index": true},
    "configuration": {"analysis.features": {"completion": true}},
    "completion_triggers": ["."], "hook": false
  }]
}
```

宿主按语言选择一个 LSP，不按包安装顺序覆盖已有选择。识别、高亮、LSP 使用独立
的 `recognition:`、`highlight:`、`lsp:` 选择键，共用用户/项目优先级与候选移除规则。
所有服务属于受信任工作区，应用级插件不能声明 LSP。

## 可选 WASM 钩子

设置 `hook: true` 后，宿主向该包的活动实例发送
`Notification::LanguageService(language::Context)`。上下文包含 provider ID、工作区根、
带来源的有效设置，以及该 provider 的固定服务候选。
插件返回 `Output.language_service: Some(language::Proposal)`：

- `service`：选择声明的默认服务或 `alternatives` 中的另一个服务；每个候选分别授权。
- `project_root`：工作区内相对目录；规范化后不能通过 `..`、绝对路径或链接逃逸。
- `program`、`args`：动态发现服务位置、计算启动参数。任一字段出现都另需 **`process.exec`**；
  只持有固定服务权限不能覆盖命令。沿用通用工具链解析，不执行拼接的 Shell 字符串。
- `initialization_options`：原样写入 LSP `initialize.initializationOptions`。
- `configuration`：配置节名称到 JSON 值的映射；`workspace/configuration` 按原名查询，
  未知配置节返回 null，宿主不追加任何具体服务器名称。

省略字段沿用声明。`executable_setting` 可指向一个声明的 string 设置；显式用户或项目值
必须是有效绝对可执行路径，并优先于发现值。显式错误报告为服务准备失败，不回退其他程序。

钩子使用普通 WASM 调用预算和内存限制，可读取已授权的包资源、工作区或私有文件；
不能写文件、启动进程、操作编辑器、订阅事件或发布 UI/快照。临时文件句柄在调用结束时
释放。钩子输出在任何场景发布前检查；初始化/配置合计受 256 KiB 声明预算限制。

## 就绪、文档与生命周期

默认完成 `initialize` 响应和 `initialized` 通知即为就绪。可声明 `readiness`：
`notification`、指向 params 的 RFC 6901 `pointer`、JSON `expected` 和 1–300000 的
`timeout_ms`；宿主读取真实通知直到匹配或超时。就绪不等于完成所有索引。

宿主提供 stdio JSON-RPC、UTF-16 位置、didOpen/didChange/didClose、可选 didSave、
诊断、定义、补全和悬浮。安装后重绑已打开文档，使用 EditorState 中的未保存文本。
文档版本在连接内单调递增，同 URI 重开不复用版本；新接口的推送诊断必须带 version，
否则忽略并依赖服务支持的 pull diagnostics，避免无法归属的迟到结果覆盖新文本。

普通请求等待 30 秒，超时发送 `$/cancelRequest` 并结束等待；取消不表示服务端副作用回滚。
停止或切换 provider 会终止其进程树、拒绝旧适配器的新请求和迟到结果。宿主保留有界
JSON-RPC 帧与消息队列；stdout 是协议流，stderr 被持续排空。

配置更改重新计算计划，只替换最终 root/program/argv/初始化/配置有变化的服务。
禁用、卸载、包替换、关闭或撤销工作区会撤销启动租约，即使旧后台任务仍持有引用也不能
再启动。重新选择仍安装的 provider 使用新的适配器生命周期。

原生程序仍以当前用户权限运行，WASM 沙箱不能限制其内部操作。服务安装和依赖下载
由独立依赖能力管理；本能力不隐式下载工具。
