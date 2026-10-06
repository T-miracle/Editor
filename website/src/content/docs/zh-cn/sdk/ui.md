---
title: 原生界面
description: ui.native 文档树、画布、事件、弹窗、主题与宿主施加的限制。
section: sdk
order: 2
alternate: /en/sdk/ui/
---

# 原生界面

底栏 `Document.tools`、窗口／文件目标、包内图标、双语提示与偏好 CAS／watch 见 SDK 随附的 `TOOLS.md`。

## 文件中心布局（editor.layout 1.0）

协商 `editor.layout ^1` 后，所选工作区文件提供者可以发布 `Document.editor_layout = true`，
用已有 Row／Column／Scroll 等原生节点组合中心内容；文本使用当前 `Document.source`，
非文本使用独立 `Document.file`。要求 `editor.read`、工作区实例及非 auxiliary 的编辑区贡献。

`Kind::NativeEditor { document: DocumentVersion }` 引用既有原生编辑会话，不创建文本或撤销栈。
该引用只允许出现在布局 root 中，至多一个，目标必须与 `Document.source` 完全一致；
工具栏、弹窗、跨文件和过期版本中的引用都会被拒绝。省略引用即可仅显示插件内容；
没有文本会话的文件不能借用其他文件的编辑器。隐藏输入目标撤销焦点，重现仍使用原会话及原生 IME 状态。

`Panel.auxiliary = true` 声明辅助文件工具，只允许编辑区贡献，不能成为中心布局提供者。
宿主按工作区＋文件类型保存提供者身份。唯一可用候选可首次采用并记忆；多个候选必须从文件标签右键菜单明确选择，
新安装不会覆盖有效选择。失效不改写用户选择；文本回退复用普通编辑器，菜单提供“恢复普通编辑界面”，
非文本保留 Tab、原因与替代查看器入口。选择变化推进目标版本，旧源、旧 UI 版本和旧实例事件继续沿既有门禁拒绝。

独立示例见 `plugins/layout-example` 与 `scripts/build-layout-example.ps1`，不依赖宿主仓库 crate 路径。
插件拥有布局、显示模式及共享偏好，宿主只验证权限、引用与资源归属并进行原生绘制。

## 文件显示与只读图片

`editor.files ^1` 提供 `Notification::FilePreview { file: Option<FileContext> }`：
文件版本含独立 ID、工作区相对路径和单调 revision；`file_type` 为规范化扩展名，
`text` 仅在文件实际具有文本能力时提供 `DocumentVersion`。关闭、切换及同路径重开
撤销旧资源；非文本文件不创建文本会话，也不借用上一文件的 revision。该入口只允许
已声明编辑区面板的工作区实例，需要受信任工作区、协商能力和 `editor.read`。

编辑区声明的 `file_extensions` 匹配文本，`readonly_file_extensions` 匹配只读文件，
总数不超过 32。后者要求 `editor.files`，已安装但停用的声明仍能识别文件模型。
没有查看器时，宿主使用通用内容识别保留非文本文件 Tab，并提供错误与重试。

`ui.file_images ^1` 的 `Kind::FileImage { alt, sizing }` 必须绑定 `Document.file`
且精确回显当前文件版本。该节点只读取当前文件，不能替换资源 URI；
受控后台读取需要 `workspace.read`，规范化路径、实例归属及工作区边界均须通过。
最多 64 图片节点，编码字节上限 8 MiB；原生解码支持 PNG、JPEG、GIF、WebP，
独立检查像素、内存预算和错误。GIF／WebP 首期静态帧。失败只影响对应资源。

`OriginalContain` 以当前内容区域计算
`min(1, available_width/image_width, available_height/image_height)`，
保持宽高比、居中、完整显示并禁止自动放大；`Contain` 是插件显式请求允许放大的适配。
插件选择显示策略，宿主通用解码、执行原生绘制；资源改变、权限撤销和实例退出均撤销旧结果。

`Document.editor_viewport` 以活动 Scroll ID 绑定源视口通知及 `Action::Viewport`。需要 `editor.viewport`、`ui.richtext`、`editor.read` 和精确 source；分栏、同步开关、来源标记、定位与撤销契约见 SDK 随附的 `VIEWPORT.md`。

`ui.code_highlighting ^1` 通过默认关闭的 `Document.code_highlighting` 请求已选动态 WASM 语言提供者高亮只读 CodeBlock；仍需 `ui.richtext`、`editor.read`、当前 source 与本实例工作区编辑区面板。缺失、失败或停用提供者降级为等宽文字，不拒绝整份合法视图。版本、取消、预算与原生主题绘制契约见 SDK 随附的 `CODE_HIGHLIGHTING.md`。

`ui.links ^1` 允许 `Document.link_events` 启用实际富文本链接事件 `Action::Link { uri }`；默认惰性，不在解析或绘制时打开地址。节点、UI revision、模态、禁用与实例归属门禁仍有效。事件与版本化 `editor.navigation` 请求的权限、目标及取消契约见 SDK 随附的 `NAVIGATION.md`。

## 可组合布局（清单 protocol 7）

`ui.native >=1.1, <2` 增加 `Layout.resizable`：仅用于有 2–16 个子节点且不换行的 Row／Column。
插件决定分栏结构；宿主复用原生尺寸状态、指针捕获与拖动行为，并绘制本地分隔线。
该声明不新增文本会话，NativeEditor 仍借用唯一的编辑状态。

`ui.content_colors ^1` 为 `Document.content_colors` 提供最多 64 项 `role.property` RGB 默认值。
键为至多 128 字节的 ASCII 字母、数字、点、下划线或短横线，值不得超过 `0xffffff`。
绘制按用户主题 `plugins[id].ui`、当前插件文档默认值、通用原生主题的顺序解析；
不读取另一插件的默认值，不改变控件的行为或宿主公共外观。终端 ANSI／Markdown 内容配色由各自包提供。

同一文本会话解析期间可以保留此前只读树和几何布局。旧树的控件事件、菜单与模态输入暂停；
已有 NativeEditor 显示当前原生文本并保留焦点。旧树不能提交编辑、定位或复用新文件资源，
新快照仍须回显精确 source/file；换文件、重开、换提供者或实例退休不得保留这条接缝。

`ui.collections ^1` 增加 `Kind::SideTabs` 与 `Document.menu`。SideTabs 是普通布局节点，可在行列树任意位置组合；节点 ID 与集合 ID 相同，条目动作仍按稳定 ID 返回。插件通过节点宽度响应 `Resize(width)`，相邻画布会得到独立的实际尺寸测量。PopupMenu 锚点相对文档，覆盖正文、不占布局空间；显示时只接受该菜单的选择或关闭，Dialog 优先。

`ui.canvas >=1.1` 增加 `Canvas.font` 和 `Canvas.scroll: Option<ScrollRange>`。字体覆盖画布继承值并参与网格测量；范围的 `content/offset` 使用逻辑像素，拖动原生滚动条产生 `CanvasEvent::Scroll { offset }`，宿主不保存第二份终端内容。字符网格的行滚轮以 `GridMetrics.cell_height` 换算，普通画布保持像素事件。

`ui.richtext ^1` 增加只读 `Kind::RichText { html }`、`Kind::CodeBlock { text, language }` 与可选 `Node.source_range`。这是独立能力：继续使用 `Document.version=1` 与 `ui.native ^1`，没有使用这些字段的包无需增加声明。未协商该能力时，任何富文本、代码块或带源码范围的普通节点都在发布前返回 `CapabilityUnavailable`。

`ui.images ^1` 增加 `Kind::Image { source, alt }` 与 `Node::image(id, source, alt)`。它继续使用 UI 文档版本 1，必须提供当前 `Document.source`；没有协商能力返回 `CapabilityUnavailable`。资源加载和富文本 HTML 的默认图片回调互相独立，详见下文授权图片资源。

文档 revision 标识交互对象及语义，不是绘制帧计数。输入目标、会话/进程身份或模态变化应递增；普通输出、绘制、按键反馈保持版本，避免同一帧的 Key/Text 或在途输入被错误判为过期。插件异步调用的后续操作应绑定最初目标的稳定身份，目标撤销后丢弃，不能跟随当前焦点。

新插件协商 `ui.native ^1`，通过 `api::Output.views` 返回 `ui::Document`。`Column`、`Row`、`Scroll`、`Tabs` 可在任意位置嵌套标准组件和 `Kind::Canvas`，没有终端专属槽位。画布另需 `ui.canvas ^1`，字符网格仅在协商 `ui.grid ^1` 并设置 `Canvas.grid=true` 时出现。UI 能力不授予文件、文档或进程权限。

`Canvas.paint` 使用通用 `Paint::Fill/Text/Svg`；SVG 由后台受限渲染器绘制，禁止环境中的文件与网络读取。每份文档最多 2048 个节点、32000 个绘制操作、16 个 SVG，序列化后最多 2 MiB。无效布局在发布之前拒绝。原生文字采用宿主字体；插件通过 `Notification::Theme` 更新自己的绘图颜色、字号，尺寸变化通过画布节点的 `CanvasEvent::Resize` 热更新。

每个 `UiEvent` 包含当前文档 revision 和节点 ID。宿主先验证活动节点、模态范围与事件类型；过期版本返回 `StaleRevision`，不存在或禁用节点返回 `InvalidHandle`，事件类型不匹配返回 `InvalidRequest`。这些被拒绝的宿主回调不会使插件崩溃。稳定节点 ID 保留输入框焦点与组合输入。交互画布须显式设置 `focusable=true`，IME 的未提交文本保留在宿主，提交后通过 `CanvasEvent::Text` 发给该画布；相邻输入框独立处理自己的输入。鼠标按钮编号为左 0、中 1、右 2，坐标相对画布节点。

编辑区预览声明 `position:"editor"` 和 `file_extensions`，使用工作区实例，申请 `editor.read` 并协商 `editor.documents ^1`。宿主发送 `Notification::Preview { document, text }`，其中 `DocumentVersion` 指向内存文档，`text` 包括未保存修改。插件必须在 `ui::Document.source` 原样返回版本；过期预览不会替换新文档。`document:None` 与空文本表示撤销当前预览。普通面板 `source` 留空。隐藏或卸载后销毁原生事件目标并移除其布局空间。

## 旧呈现契约退役与包内工具图标

`editor.presentation`、`Panel.view_modes`、`PreviewMode`／`PreviewModes` 和
`Installed::preview_mode_icon` 已移除。使用当前 SDK 将中心内容改为 `editor.layout`，
用 `Kind::NativeEditor` 引用唯一编辑会话，省略引用即可只展示插件内容。
`Command.toolbar`／`toolbar_icon` 同样已移除；命令继续提供菜单、快捷键与调用入口，
插件功能按钮通过 `ui.tools` 的 `Document.tools` 提供，详见 SDK 随附的 `TOOLS.md`。
Panel 与 Command 拒绝未知字段；含退役字段或要求退役能力的包明确提示更新，宿主不解释旧业务。
有限旧显示偏好导入只写入已授权插件私有记录，不恢复旧运行协议。

工具图标从 `ToolIcon.light`／`dark` 的包内相对路径读取，不能使用宿主资源名称或跨插件路径。
每个 SVG 最大 64 KiB，XML 树最多 512 节点、几何元素最多 16 层嵌套；
只允许 `svg/g/path/rect/circle/ellipse/line/polyline/polygon` 与几何、描边和颜色属性。
允许颜色名、十六进制颜色和 `currentColor`；拒绝 CSS、URL paint、`href`、图片、文字、脚本、DTD 与 XML 样式表。
发布工具时验证字节及归属；`Installed::tool_icon(root, path)` 在绘制读取时再次校验当前版本目录，
不存在、不安全、超限、路径或 junction 越界均返回 `None`，不授予访客额外文件或网络权限。
实例／场景失效撤销工具事件与资源，旧只读树可以保留绘制，但不能执行排队定位。

独立 SDK 示例见 `plugins/capability-example/src/composition.rs` 与同目录 `composed-ui.json`；将示例的 `label` 配置为 `composable-ui` 可展示组合界面，`ui-layout` 命令参数 `form/canvas/combined` 切换三种布局。

## 源码工具栏（editor.toolbar 1.0）

`Document.editor_toolbar: Option<Node>` 在源码编辑区上方提供通用原生节点树；预览正文仍由 `root` 表达。工具栏必须提供当前不可变 `Document.source`，协商 `editor.toolbar ^1`，申请 `editor.read`，并属于当前工作区实例自己声明的 `position:"editor"` 面板。没有源码版本返回 `InvalidRequest`；未协商能力返回 `CapabilityUnavailable`；面板所有权、作用域或读取授权不满足则返回 `PermissionDenied`。该能力只贡献控件，不直接授予编辑权限。

工具栏、root、dialog 和 menu 共用节点身份、文本、节点数、深度与编码预算；工具栏中的其他可选节点也须协商对应能力。工具栏事件仍以 `api::View.panel` 为所属面板，携带同一 `UiEvent.revision`、节点 ID 和动作。仅显示源码时仍可响应；仅预览时随源码区隐藏。Dialog 和 PopupMenu 保持输入优先级，禁用节点与过期回调沿用现有门禁。撤销预览版本或停用插件时移除工具栏及其事件目标。

通用 `Node.tooltip: Option<String>` / `.tooltip(text)` 携带随 `Environment.locale` 本地化的提示，原生按钮将其映射为悬浮说明与可访问标签，计入现有 UI 文本预算。`Layout.wrap: bool` 默认 false；行节点可 `.wrap()` 在窄宽度下换行，宿主根据实际内容计算高度。工具栏外层和分组可使用可换行 Row，按钮保留最小尺寸，不设置固定工具栏宽高。

## 常用界面元素

| Kind / 构造方法 | 用途 | 事件 |
| --- | --- | --- |
| `Column` / `Node::column` | 纵向自动布局 | — |
| `Row` / `Node::row` | 横向自动布局 | — |
| `Scroll` / `Node::scroll` | 带原生滚动条的纵向滚动区，设置 height 约束视口 | — |
| `Text` / `Node::text` | 普通文字，按容器换行 | — |
| `RichText` / `Node::rich_text` | 只读原生富文本，输入为受限 HTML 标记，需 `ui.richtext` | — |
| `CodeBlock` / `Node::code_block` | 保留空白和换行的只读等宽代码，需 `ui.richtext` | — |
| `Image` / `Node::image` | 按版本与权限异步读取并原生呈现图片，需 `ui.images` | — |
| `Button` / `Node::button` | 原生按钮，支持键盘激活 | `Click` |
| `Input` / `Node::input` | 单行原生编辑，支持中文 IME、选区、撤销与复制粘贴 | `Change(String)`、`Submit(String)` |
| `Checkbox` / `Node::checkbox` | 复选框 | `Toggle(bool)` |
| `Choice` | 带稳定选项 ID 的单选组；选项可禁用 | `Select(option_id)` |
| `Tabs` | 标签条与当前页内容 | `Select(tab_id)` |
| `List` | 只读文字列表 | — |
| `Table` | 只读文字表格 | — |
| `Separator` | 分隔线 | — |
| `Progress` | 0–100 的确定进度条，带无障碍标签 | — |
| `Spacer` | 占位空间，可设置宽高或 grow | — |
| `Document.dialog` | 复用主程序原生控件的模态浮层 | `Dismiss` |

`Node::new(id, Kind::...)` 可构造全部类型。`gap/padding/width/height/grow/role/disabled` 构造方法对应公开的 `layout/role/disabled` 字段。`source_range(start..end)` 对应可选源码映射，默认留空。布局尺寸为逻辑像素。`disabled` 作用于整个子树；隐藏标签页和模态弹窗背后的节点不会收到操作事件。

## 只读富文本与源码块映射（ui.richtext 1.0）

插件负责领域解析，把标题、段落、强调、删除线、列表和表格转换为受限 HTML，再交给 `RichText.html`。宿主只用原生富文本控件排版，不解析 Markdown，不使用 WebView，不执行脚本或任意 CSS。原文中的内嵌 HTML 不获得执行入口；插件应过滤或转义不支持的原始标记。颜色、字体与间距由本地主题决定。

插件生成的提示、图片替代说明和空状态应跟随 `Environment.locale`，同时保留中英文。该字段在 Prepare 时提供，并随 `Notification::Theme` 更新；缺失或空值沿用既有简体中文默认，不从工作区文件推断用户界面语言。

富文本控件必须覆盖默认链接点击与图片资源加载回调：出现 `href` 或 `src` 不自动打开浏览器，不读取本地文件、网络或 data URL，也不借用宿主加载器绕过插件权限。这一能力本身不提供链接导航、图片访问或预览写入。授权图片使用独立 Image 节点，缺少该能力时插件以替代文字表示图片。任务框通过普通原生 `Checkbox` 节点表达；只读任务应设置 `disabled=true`，不能依赖 HTML `<input>` 自动变成可交互控件。

需要链接交互时另行协商 `ui.links`，显式开启 `Document.link_events`，并用 `Node.links` 提供图片外层链接与文字键盘目标。声明不会获得浏览器或文件授权；版本化事件、原生焦点与共享配额见 SDK 随附的 `NAVIGATION.md`。

`CodeBlock.text` 是无需转义的原始代码，空白与换行按原样显示；可选 `language` 是 1–100 个 ASCII 字母、数字或 `._+-#` 的标识，允许 `c++`、`c#` 等名称。单独声明语言不启动工具或授予高亮能力；基础呈现为等宽文字，显式开启 `Document.code_highlighting` 后才经独立能力复用已选动态 WASM 高亮提供者。插件负责把业务别名归一为提供者的公开语言身份，未知或不可用语言继续显示等宽代码。

每个源码块使用稳定的 `Node.id`，`Node.source_range = { start, end }` 表示半开 UTF-8 字节范围。范围针对 `Document.source` 绑定的不可变内存文本版本，不能用生成的 HTML 字节偏移代替。任何类型的节点都可带映射，但必须协商 `ui.richtext` 并提供 `Document.source`；未提供版本或 `start > end`、`end > 1 MiB` 返回 `InvalidRequest`。宿主使用映射前还应对当前源文本核对长度与字符边界；旧文档身份或 revision 仍按现有预览门禁返回 `StaleRevision`。

映射只携带身份与偏移，不保存第二份可变文档，也不构成编辑授权。HTML 控件不保留原始 Markdown 的源码范围；布局定位必须使用协议中的块 ID 与范围，在块重排、文档修改或视图撤销后相应更新、丢弃。

## 授权图片资源（ui.images 1.0）

插件发布 `Node::image("photo", "../assets/photo.png", "照片说明")`，将同一次 Preview 的 `DocumentVersion` 原样放入 `Document.source`。Image 是工作区编辑区预览的声明式资源，宿主校验当前实例与面板归属，再在后台读取和解码，不把图片大字节经 JSON 返回 WASM。`source` 为 1–4096 个 UTF-8 字节；`alt` 由插件提供，作者文本原样保留，缺省文案按 `Environment.locale` 本地化。两者计入普通 UI 文字预算。root、toolbar、dialog 合计最多 64 个图片节点，超限或缺 source 返回 `InvalidRequest`。图片节点附加源码范围时仍须另外协商 `ui.richtext`。

本地 URI 以源文件所在目录为基准，先进行一次 percent decoding，再拒绝绝对路径、反斜杠、设备名称、冒号和备用数据流。允许 `../`，但源文件目录和图片最终规范化路径必须仍位于当前实例所属工作区内；符号链接和 Windows junction 的实际目标同样检查。必须声明并批准 `workspace.read`，不能借用私有数据、其他工作区或其他插件的访问权。

远程 URI 只接受无凭据的 `http://` / `https://`，必须声明并批准 `network.images`。请求不跟随任何重定向，不读取环境代理、不附加认证信息或 Cookie；`file:`、`data:`、其他 scheme 和凭据 URL 不进入宿主通用图片加载器。每张编码图片最多 8 MiB，manager 编码驻留总额最多 64 MiB；进程内所有 manager 共用最多 8 个工作线程。排队及读取共用 30 秒期限，HTTP 解析、连接和流读取都受此期限限制；超时为终态，迟到 body 不会复活成功。

缺少图片对应权限、路径不存在或越界、HTTP 错误、超额和解码失败只影响该图片。宿主显示该节点 `alt` 与本地化原因，继续呈现其他文字与图片。加载和失败均不触发整篇视图拒绝；协议结构和能力协商错误仍在发布之前拒绝。

宿主通过公开 runtime `Manager::image_resources(&mut self)` 获取 `BTreeMap<String, Arc<ImageResource>>`，key 为 `plugin/panel/image/node_id`。`ImageResource { source: DocumentVersion, uri: String, state: ImageState }` 的状态为 `Loading`、`Ready(Arc<Vec<u8>>)` 或 `Failed(api::Failure)`；原生层只将 Ready 的有限字节交给安全解码器，不能把 URI 直接交给会读取环境文件或网络的原生控件。PNG、JPEG、GIF、WebP 与受限 SVG 的支持由原生解码器提供；SVG 的外部文件、网络和脚本仍无执行或读取入口。

原生缓存共用 64 MiB 像素额度、单轴至多 4096。SVG 仅支持 UTF-8 XML，拒绝 SVGZ 与 DTD；XML、深度、引用展开（含文字／几何载荷）及 paint 定义复用在转换前检查，临时画布与效果采样工作量在绘制前检查。配额失败在可用容量增加前保留结果，避免后台轮询重复重解码。已绘制图片由有效原生投影计数，最后一个投影退休时显式清除 GPUI atlas；共享投影不能提前清除仍在使用的图片。

任务同时绑定实例 incarnation、面板、源码身份与 revision、节点和 URI。节点替换、Preview 撤销、文档推进或关闭、停用/卸载、实例更新和工作区切换都会断开旧消费者；取消不等待后台 IO 完成，旧结果与字节额度随其消费者/生产者释放。资源是不可变发布快照，不保存第二份可变源码、撤销栈或图片磁盘缓存。

系统 DNS 无法物理中断时仍在已计数的 worker 内执行；消费者到期即失效，迟到 DNS 不再连接网络，不创建额外、未计数的解析线程。

HTTP(S) scheme 按 ASCII 忽略大小写，因此 `HTTP://` 与 `HtTpS://` 仍使用网络授权；资源身份与 `ImageResource.uri` 保留插件原样声明的 URI。

## 身份、状态与事件

- 节点 ID 在一个面板的文档（包括所有标签页和弹窗）内唯一且稳定，允许 ASCII 字母、数字、点、下划线、连字符。主程序按 ID 保留输入状态和滚动位置；不要每帧生成随机 ID。
- 每次插件响应可以提交一份完整文档，主程序按 ID 协调原生状态。`revision` 由插件维护并随事件返回，插件可据此辨认旧界面产生的操作；事件不是 GPUI 回调或内存指针。
- 事件外层是 `api::Input::Event { panel: Some(panel_id), event: api::Notification::Ui(...) }`。`UiEvent.node` 为节点 ID，`action` 为类型化事件。选项和标签返回稳定 ID，不返回数组下标。
- 输入框的 `value` 是初始值。`value_revision` 不变时，主程序保留正在编辑的草稿，避免较慢的插件回复覆盖新输入。插件响应 `Change` 更新业务状态即可；清空、加载或重置输入时，增加 `value_revision` 才会替换原生草稿。程序性替换不会触发 `Change`。
- 按钮与选择类控件由插件状态控制：收到事件后返回新的 `checked/selected`。插件卸载、替换或移除节点时，原生输入与订阅随视图释放。

## 弹窗

```rust
use plugin_protocol::ui::{Dialog, Document, Node};
let document = Document::new(Node::button("open", "打开"))
    .dialog(Dialog::new("settings-dialog", "插件设置",
        Node::column("settings-body", vec![
            Node::text("hint", "弹窗内容也使用同一套节点"),
            Node::button("save", "保存"),
        ]).gap(8.)
    ));
```

点击关闭或按 Escape 返回 `Dismiss`（node 为弹窗 ID）；插件应将下一份文档的 `dialog` 设为 `None`。确认按钮的语义由插件定义。等待插件回复期间继续保持模态，重复关闭请求只发送一次。弹窗关闭后恢复之前的焦点。协议 v1 每个面板最多一个模态浮层，尚不提供任意系统顶层窗口或嵌套弹窗。

## 源码图片输入（editor.images 1.0）

`Document.editor_image_input: bool` 默认 false，开启后在该文档源码区接收原生图片粘贴和外部文件拖入。必须携带当前 `Document.source`，协商 `editor.images ^1`，具有 `editor.read` / `editor.write`，且属于工作区实例自己声明的 editor 面板；普通面板、application 实例或缺少授权在发布前拒绝。源码单栏与左右分栏共享同一输入授权，仅预览模式不接受源码编辑输入。

真正捕获需要 `workspace.write`，剪贴板来源还需要 `clipboard`。宿主校验实际图片格式和有限批量，将只含元数据的 `Notification::ImageInput` 返回同一 `api::View.panel`；宿主保存编码字节，访客选择 basename 并提交 `SaveImageInput`。这项声明不提供任意路径读取、环境剪贴板轮询或 JSON 图片上传。撤销声明、文档/实例变化或 30 秒到期清理未使用资源；已受理保存的成功回执保留，源变化不能把引用插入到新的目标。

每批最多 8 张、单张最多 8 MiB、每批最多 32 MiB，宿主待保存编码字节最多 64 MiB。
`SaveImageInput { input, name }` 仅使用原子 create-new 创建源文件同级附件。名称为最多 255 字节
的安全 basename，后缀必须对应实际图片格式；目录、设备名、ADS、末尾空格和点均拒绝。
已有文件返回 Conflict 且不消费句柄，插件可以换名重试；只有匹配请求的成功回执才消费输入。
开始文件副作用后的取消仅停止等待，不回滚外部文件，迟到成功不能改写取消终态。

文件保存与引用编辑分别完成。插件等待保存回执后以一次版本化 `editor.edit` 事务插入引用；
过期目标拒绝插入，已完整保存的附件保留；文本 Undo 仅删除引用。宿主只清理该请求创建的
不完整文件。这项能力授权同级附件写入，不提供任意工作区写入。图片预览独立使用 `ui.images`
的受控资源，不执行 HTML 默认图片读取。

## 主题和字体

节点只声明样式角色，没有硬编码颜色或字体字段。省略 `role` 时，使用 `button/input/checkbox/choice/tabs/text/rich_text/code_block/image/list/table/progress/separator/scroll/spacer/container`；弹窗外壳使用 `dialog`。

主题在 `themes[].plugins[插件ID]` 中配置，示例：

```json
{
  "ui": {
    "button": {
      "background": "#243044",
      "foreground": "#FFFFFF",
      "hover_background": "#33455F",
      "active_background": "#405777",
      "border": "#62738A"
    },
    "choice": { "accent": "#66B3FF", "accent_foreground": "#101820" }
  },
  "typography": {
    "button": { "family": "Segoe UI", "size_px": 14, "bold": true }
  }
}
```

各角色支持 `background/foreground/border/hover_background/active_background/accent/accent_foreground` 中适用于该控件的颜色。未声明项继承编辑器当前主题。字体角色继承主题 UI 字体。主程序切换主题时直接重绘已有树，同时继续发送 `Notification::Theme` 给插件。

## 约束与兼容

- `Document.version` 当前必须为 1；构造器自动设置。
- 单文档至多 2048 个节点及展开后的选项、列表行和表格单元格，嵌套深度至多 24。富文本每个 `<` 标记与代码每行也保守计入展开节点额度，避免用一个协议节点制造无界原生布局。单段文字、富文本 HTML 或代码文字至多 64 KiB，总文字至多 1 MiB（包含代码语言标识），完整文档编码至多 2 MiB；富文本不会获得独立的额外额度。
- 宽高必须为有限的 0–10000 数值；间距和内边距为 0–256；弹窗宽度为 240–1200；表格为 1–32 列，每行列数一致。
- 重复 ID、无效选中值和未知协议会被拒绝。一次 Output 的所有文档先验证，再一起发布，避免部分界面更新。
- Canvas、SideTabs、只读富文本和标准组件使用同一棵节点树；尚不提供任意 GPUI 对象、富文本编辑器或虚拟表格。旧 Scene/Widget/controls/chrome 传输已删除，旧包在安装与恢复前明确拒绝。

验证协议可运行 `Document::validate()`；SDK 随附契约测试。主程序另有原生点击、输入、弹窗与实际 WASM 包回归测试。
