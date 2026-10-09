---
title: 文档与只读资源
description: 读取当前会话、观察事件，并比较提供者拥有的文本。
section: sdk
order: 27
alternate: /en/sdk/documents/
---

# 文档与只读资源

`editor.documents` 1.1.0、`editor.virtual` 1.0.0 与 `editor.diff` 1.0.0 是稳定核心能力，目前均未弃用，兼容、弃用与协商遵守 [SDK 稳定性政策](/zh-cn/sdk/#稳定性与能力兼容)。本页新增操作要求宿主支持 `protocol = 7`、`api.base = ^1` 与下文各能力的最低范围。将需要的新增能力声明为必需，或声明为可选并在缺失时显式禁用或替换相应功能；未协商的操作不可调用。这些新增能力不恢复任何旧传输协议。

协商 `editor.documents ^1.1` 并声明 `editor.read`，通过 `guest::list_documents`、`guest::read_document` 返回的 `EditorTask` 消费匹配的 `Notification::Request` 和最终 `EditorValue`。关闭后重新打开产生新文档 ID；资源使用规范化本地身份或实例拥有的虚拟句柄。身份本身不授予文件或其他实例的访问权限。

`ListDocuments` 最多返回 128 个可读文本会话与活动版本，覆盖后台文档和未保存内容。每份 `DocumentInfo` 包含 read/edit/save 能力、dirty、标题、语言、编码、EOL、字节长度、选区和已布局的可见行。其他实例的虚拟文档被排除；若它是活动文档，活动身份返回空。本地身份使用工作区相对的普通路径，不进行 URL 百分号解码，`#` 和 `%` 保留为文件名字符。

`ReadDocument { document, range }` 同时校验 ID、路径和 revision，在原生编辑器上取得同一版本的不可变文本与元数据，不重新读盘。省略范围表示全文，每次返回最多 256 KiB；大文档按同一版本分段读取。编辑、重命名、刷新或关闭后，旧版本返回 `StaleRevision`；撤销或跨实例的虚拟引用返回 `InvalidHandle` 或 `PermissionDenied`，不会通过猜测路径获得权限。

## 坐标

`DocumentRange::Bytes` 使用半开 UTF-8 字节范围，端点必须位于字符边界。`DocumentRange::Utf16` 与 `TextPosition` 使用从零开始的源行及 UTF-16 列；CRLF 视为一次换行，CR 与 LF 也分隔源行，代理对不能拆开。越界或非法端点返回 `InvalidRequest`，不静默截断。UTF-8 BOM 若存在，仍计入文本和坐标。可见行是原生布局行，受折叠和换行影响，并非源行号。

例如 `中😀\r\nsecond` 的第 0 行 UTF-16 列 1..3 对应字节 3..7，第 1 行列 0 对应字节 9；列 2 拆分 emoji，属于非法位置。

## 事件和释放

`guest::subscribe_document_events()` 显式订阅 `Notification::DocumentEvent`，与原有 `SubscribeDocuments` 共用每实例八个订阅的上限，通过 `guest::close_resource` 释放。旧订阅保持本地路径 `DocumentChange` 及最新 revision 合并语义，协商到 1.1 不会自动收到新通知变体。旧选区和预览接口不暴露虚拟内容。

详细事件包含打开、文本变化、关闭、保存前、保存后（含失败）、活动文档、选区和视口。顺序号在宿主工作区会话内递增；原生观察可能合并中间选区及视口测量，FIFO 保留全部已发布观察及保存/关闭顺序。`WillSave` 只是观测，可能在写盘后才送达，不能阻塞、取消或拦截保存。事件不含文本 delta，按事件的精确版本补读，并忽略旧版本。

入口和每个订阅最多保留 128 个事件。溢出清空不完整队列，以 `SubscriptionFailed(LimitExceeded)` 明确终止并释放句柄；重新枚举当前状态并重新订阅，不把部分事件当成完整历史。回调中释放自己也会立即停止后续投递。信任撤销、工作区关闭和实例退役会统一撤销权限。

## 只读提供者内容

协商 `editor.virtual ^1` 并声明 `editor.read`。`OpenVirtualDocument { title, language, text }` 打开普通原生只读 Tab，返回 `DocumentOpened(DocumentInfo)`，无需临时文件。每实例最多 32 份资源，每份最多 1 MiB。`RefreshVirtualDocument` 仅替换自己拥有的精确版本，推进 revision，并清理只读显示的选区/Undo，不产生用户编辑。language 是显示提示；只读资源不启动原生语言服务。

`OpenDocument { resource }` 激活已有虚拟资源或打开本地文本文件；本地打开还需要 `workspace.read`，并重新核对规范化物理路径位于工作区。`LocateDocument { document, position }` 在两类资源上聚焦并选择合法 UTF-16 位置。

虚拟内容明确提供 `edit=false`、`save=false`，输入、粘贴和普通保存被拒绝，会话层也不能调用文件写入。虚拟 Tab 不进入文件监视、本地历史或文件恢复记录。释放句柄、关闭 Tab、禁用/切换插件或关闭窗口撤销同一 authority，宿主清理保留的原生视图。候选准备失败保留仍有效的旧实例；成功替换产生新句柄，不恢复序列化的旧资源 ID。

## 比较与示例

协商 `editor.diff ^1` 并声明 `editor.read`，用两个精确版本发送 `CompareDocuments { left, right }`。原生比较借用现有 EditorState，标记新增/删除/修改行，将左侧显示限制为只读，两侧独立滚动。左侧聚焦时保存不能误写活动右文件；右本地文档保留正常编辑与保存。任一源变化/关闭、离开活动右目标或提供者撤销后，比较和它自己拥有的装饰层被清理。

比较每侧最多 1 MiB、2000 行，LCS 矩阵最多 4M 格，超限返回 `LimitExceeded`；不增加第二份可变文档、工具页或可写虚拟文件系统。

独立项目 `plugins/history-preview` 展示固定历史内容，`plugins/generated-preview` 从当前未保存快照生成建议。使用宿主 `--plugin-package <项目> --output <目录>` 构建，类型来自 `--plugin-cargo` 使用的同一 SDK 缓存。插件命令菜单提供 **Compare historical content**、**Refresh historical content** 和 **Preview generated text**，重复运行可刷新只读资源并导航/比较新版本。
