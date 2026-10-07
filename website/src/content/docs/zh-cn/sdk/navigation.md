---
title: 链接与导航
description: 公开能力、权限与生命周期。
section: sdk
order: 13
alternate: /en/sdk/navigation/
---

# 原生链接与版本化导航

`ui.links ^1` 与 `editor.navigation ^1` 是独立的公开能力，基础协议仍为 7、UI 文档版本仍为 1。二者不授予文本编辑、网络下载或进程执行能力；包版本单独提升。

## 用户链接事件

声明 `Document.link_events=true` 必须已协商 `ui.links`。原生 `RichText` 使用 Base 的实际链接命中与选择行为，用户激活链接后发送 `Action::Link { uri }`。右键及拖选不触发导航；解析、布局、主题更新和图片加载不产生链接事件。未声明时链接保持惰性。事件仍经过节点、UI revision、模态归属、禁用状态与活动实例检查，URI 最多 4096 字节，不能含控制字符。

链接事件只是请求意图。插件须确认该 URI 属于当前解析结果，并选择具体业务行为；宿主不判断 Markdown、标题 slug、扩展名或插件身份。原始 HTML 不得到执行或隐式文件／浏览器入口。

`Node.links: Vec<LinkTarget { uri, label }>` 为只读 `RichText`、`Image` 或图片替代 `Text` 声明原生键盘目标，发布时同样必须协商 `ui.links`。RichText 的 URI 是实际渲染 href，不是再次解码后的字符串；插件验证后再使用原始解析目标导航。每个图片／替代文字最多一个外层链接，RichText 可声明多个，保留出现顺序。标签最多 256 UTF-8 字节，空标签由宿主以当前语言提供“打开链接”。每个目标占用全树 2048 控件预算，URI 与标签计入共享文字额度。

`link_events` 默认 false，即使有目标声明也不激活。开启后，图片的整个原生内容可点击及通过 Tab、Enter、Space 操作；文字保持原生选择与指针命中，在 Tab 聚焦具体目标时显示对应链接按钮和焦点提示。隐藏键盘目标不接受鼠标激活。目标移除释放焦点资源，场景替换不能借用旧按下手势；图片布局变化不会把释放操作误判为新链接点击。

## 通用导航请求

`EditorOperation::NavigateDocument { document, target }` 要求 `editor.navigation`、`editor.read` 和活动工作区作用域。`document` 是产生请求的实际文档身份、工作区相对路径和 revision；效果前必须仍为当前活动文档。调用采用既有受控编辑器队列、请求取消／超时与 `enter_side_effect` 门禁，不写入文本、选区或 Undo。

| `NavigationTarget` | 授权与效果 | 成功值 |
| --- | --- | --- |
| `PreviewNode { panel, node, ui_revision }` | 当前插件声明的编辑区面板；当前可见预览、精确 source／UI revision、活动源码范围与最近原生 Scroll 归属。按目标下一次真实布局定位；文档或场景替换清除待定位。 | `Unit` |
| `RelativeDocument { path }` | 另需 `workspace.read`。源文件父目录相对 URI，经物理规范化检查仍处于所属工作区，并为现存文件；调用既有文档打开入口。 | `Opened { document }`，为实际打开的目标身份与 revision |
| `ExternalUrl { url }` | 另需 `navigation.external`。只接受有主机、无账号密码的 HTTP(S) URL，通过系统浏览器入口打开。 | `Unit` |

`PreviewNode` 的 panel/node 最多 100/128 字节，必须为本实例资源。`Node.source_range` 必须落在实际当前 UTF-8 文本边界内。没有活动滚动归属、隐藏面板、模态遮挡或尚未绘制的预览返回失败，不能路由到另一面板或文档。

相对 `path` 最多 4096 字节，不附查询和锚点；插件分别解析业务片段。公开 `document_relative_path`／`decode_uri_component` 严格按 UTF-8 解码百分号一次，保留字面 `+`；传入的请求保留原始 URI。允许 `..`，但最终物理目标不能逃出工作区。绝对路径、反斜杠、驱动器、备用数据流、设备名、控制字符及 Windows 非法文件字符被拒绝；符号链接／junction 必须再次通过规范化边界。`a%23b.md` 可以表示带字面 `#` 的文件名；`%252e` 不解码第二次。目标缺失或不可读不打开其他文档。

跨文件锚点由插件等待实际 `Opened` 回执和精确目标预览后再发 `PreviewNode`，不得猜测目标身份、沿用旧 revision 或对第三个文档定位。取消、切换、关闭、禁用、撤权和实例退役均沿既有请求生命周期处理；`Accepted` 不代表效果完成。

非法目标返回 `InvalidPath`／`InvalidState`，资源缺失返回 `NotFound`，过期文档／场景返回 `StaleRevision`，缺能力／权限返回 `CapabilityUnavailable`／`PermissionDenied`，取消和超时返回 `Cancelled`／`TimedOut`。其他协议不能转换为 Shell 命令；浏览器启动不授予 `process.exec`，也不下载 URL 内容。
