---
title: 语言服务
description: language.lsp 能力、可选发现钩子、就绪与文档生命周期。
section: sdk
order: 5
alternate: /en/sdk/lsp/
---

# 语言服务

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
`language.lsp >=1.2` 还提供 `package_root` 和 `data_root`：当前包版本资源与隔离私有数据
作用域的规范化原生路径。在准备候选及切换后重新计算；用于配置原生资源和缓存位置，
不得作为持久化身份。路径字符串不授予 WASI 文件访问权限，WASM 沙箱也无法限制
已获授权原生服务的文件访问。
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

必需能力声明 `dependencies >=1.1` 后，提供者可显式设置 `optional_installation: true`。
首次安装时，该提供者依赖准备失败仍允许资源功能激活；语言服务报告不可用，没有有效
准备缓存时不能启动。取消仍终止安装。更新继续要求准备成功，失败时保留旧版本。
重新安装包可重试准备，显式指定本机可执行文件可避免下载。

钩子使用普通 WASM 调用预算和内存限制，可读取已授权的包资源、工作区或私有文件；
不能写文件、启动进程、操作编辑器、订阅事件或发布 UI/快照。临时文件句柄在调用结束时
释放。钩子输出在任何场景发布前检查；初始化/配置合计受 256 KiB 声明预算限制。

## 纯快照补全

工作区语言提供者声明必需能力 `language.completion: ^1`、`editor.read` 权限和
`completion_hook: true` 后，可补充原生服务的建议。宿主向独立纯执行器发送
`Notification::LanguageCompletion(language::CompletionRequest)`。`source` 包含
`DocumentVersion`（已打开编辑区 ID、工作区相对路径、revision）及最多 1 MiB 的 UTF-8
`text`，`cursor` 为 UTF-8 字节偏移；另有 `provider`、单调的 `request` ID 和有效
`settings`。请求编号与文档 revision 分别校验，不提供可编辑文档句柄。
`uri` 为文档的标准逻辑 URI（最多 8192 字节），不含原生快照别名。可选 `diagnostics`
包含已证明属于同一文本的标准 code/message 摘要；None 表示尚未知。最多 128 条，
`code` 为整数或最多 128 字节的字符串，`message` 是最多 1024 字节的 UTF-8 预览。
宿主不解释语言专用错误；插件可区分规则加载失败与文档内容错误。

返回 `Output.language_completion`，回显同一请求和文档元数据，最多 256 项；每项含
`label`（256 字节）、包含光标的 UTF-8 `replace` 范围与 `new_text`（4096 字节）。完整
JSON 回复上限为 256 KiB。宿主检查字符边界并转换为 UTF-16，原生候选在前，同名或
相同编辑去重；返回空列表则沿用原生结果。

独立执行器不恢复私有快照、不激活资源，从首次准备指令起拒绝全部宿主操作，包括资源、
文件、进程、编辑与订阅；不能发布 UI、配置或私有快照。沿用普通 fuel、1 秒和 256 MiB
限制。补充失败保留原生结果并记录真实错误；提供者撤销时清理执行器、拒绝迟到回复。
编辑器发布前核对当前文本、已打开实体与 revision。已关联 Schema 的文件不提供无约束
历史建议，已观察到规则加载失败后保留基础建议等语言策略由插件决定。

## 就绪、文档与生命周期

默认完成 `initialize` 响应和 `initialized` 通知即为就绪。可声明 `readiness`：
`notification`、指向 params 的 RFC 6901 `pointer`、JSON `expected` 和 1–300000 的
`timeout_ms`；宿主读取真实通知直到匹配或超时。就绪不等于完成所有索引。

宿主提供 stdio JSON-RPC、UTF-16 位置、didOpen/didChange/didClose、可选 didSave、
诊断、定义、补全和悬浮。安装后重绑已打开文档，使用 EditorState 中的未保存文本。
文档版本在连接内单调递增，同 URI 重开不复用版本；新接口的推送诊断必须带 version，
否则忽略并依赖服务支持的 pull diagnostics，避免无法归属的迟到结果覆盖新文本。
提供者声明 `diagnostic_snapshots: true` 时必需 `language.lsp >=1.3`。宿主为每个不可变
文档 revision 生成独立 wire URI，仅接受当前有效映射的无版本推送；替换和关闭时撤销
旧快照，连接内不复用 URI，物理本地路径和正常导航目标保留原身份。file URI 在卷根后
插入 `language::SNAPSHOT_URI_SEGMENTS`（64）个冗余点段，以百分号编码大小写区分
请求。按绝对路径 glob 匹配的原生服务可由插件补充同等的 64 个 `./` 段，basename 和
后缀 glob 不变；保持路径边界、不扩大匹配或文件访问权限。原生服务须支持该显式选择的策略。
导航仅调用服务宣告的标准 `definition`/`typeDefinition`；定义结果为空时才回退类型定义。
类型和 Schema 目标仍经同一文档租约与 revision 检查。

普通请求等待 30 秒，超时发送 `$/cancelRequest` 并结束等待；取消不表示服务端副作用回滚。
停止或切换 provider 会终止其进程树、拒绝旧适配器的新请求和迟到结果。宿主保留有界
JSON-RPC 帧与消息队列；stdout 是协议流，stderr 被持续排空。

配置更改重新计算计划，只替换最终 root/program/argv/初始化/配置有变化的服务。
禁用、卸载、包替换、关闭或撤销工作区会撤销启动租约，即使旧后台任务仍持有引用也不能
再启动。重新选择仍安装的 provider 使用新的适配器生命周期。

原生程序仍以当前用户权限运行，WASM 沙箱不能限制其内部操作。服务安装和依赖下载
由独立依赖能力管理；本能力不隐式下载工具。
