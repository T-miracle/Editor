# Markdown 插件总方案

日期：2026-10-03。稳定标识：`markdown-plugin`。

状态：全部 11 张工单实现、验收、独立审查和 Git 交付已完成；2026-10-04 最终独立读回 #27–#37 全部 closed / completed。Markdown 0.11.0 的 M01–M16 完整正式包原生组为 62/62、0 ignored，提交、推送及关闭证据详见[最终验收](verification/11-distribution-acceptance.md)。总方案 [#26](https://github.com/T-miracle/Editor/issues/26) 保持 open。验收更新日期：2026-10-04。

主项目后续集成：完整宿主实现与 Markdown 0.11.1 已合回 `C:/Projects/RustProjects/Editor` 的 `main`；SVG 0.3.0 同样使用三个底栏视图按钮。本次构建、回归与实际产物另见[主项目集成验收](verification/12-main-project-integration.md)，不把 0.11.0 的历史全量结果冒充本次执行。

主项目后续布局缺陷：工具栏下方的源码输入区曾退化为一行；已补齐通用同步滚动观察容器的纵向高度传递，专项回归直接检查实际输入高度。配套验证与宿主产物见[源码高度修复记录](verification/13-source-editor-height.md)。

2026-10-04 追加需求已完成：十三个格式按钮统一使用图标并缩小工具栏；底栏 Markdown 面板开关采用 Toolbar 图标并移至同步滚动右侧；同源码版本启动刷新保留手动滚动位置，修复首次打开后的回顶。Markdown 0.12.0 与配套宿主已在主项目构建，18 项定向原生回归及仓库门禁通过，详见[图标工具栏与首次滚动记录](verification/14-icon-toolbar-and-first-scroll.md)。

依据：本轮 grill-me / grilling 访谈，按 to-spec 模板整理。遵守[插件平台规格](../../../docs/plugins/specs/plugin-api-platform.md)与[项目需求基线](../../../docs/project/需求整理.md)。按用户明确要求，本插件总方案、工单与验收资料保存在插件目录内，中央插件文档仅维护入口。链接按主项目当前归档位置维护。参见[工单目录](tickets/README.md)与[AI 执行入口](../AGENTS.md)。

2026-10-05 追加修改已完成：GitHub 深浅预览配色和行内代码前景、透明格式按钮、H1～H6 当前行标题切换、底栏只切换顶部工具栏、小幅同步滚动不回弹。Markdown 0.13.0 与配套宿主已构建；本轮 22 项定向原生回归、65 项插件单元、公开接口与仓库检查通过，详见[六项修改验收](verification/15-github-headings-toolbar-scroll.md)。

2026-10-05 用户报告的三个预览缺陷已完成：预览表格的 Markdown 文本缺失（单元格内的行内代码等富文本不再合并为单个 HTML 表格文本块，改为每个单元格一个原生富文本块，并按单元格身份独立布局）、输入时整个编辑与预览区反复重建（同一文件的已发布预览不再因新 revision 被撤回，一次在途发布吸收连续输入，输入停止后仍补发最新文本）、首次滚动预览被强制回到顶部（源码仍在顶部的重排通知不得覆盖用户刚滚动的一侧，只有访客收到该手动位置后源码才重新驱动预览）。Markdown 0.14.0 与配套宿主已构建，详见[三缺陷修复验收](verification/16-preview-table-typing-scroll.md)。

## Problem Statement

2026-10-06 平台规格原文约 38 KiB 却触发预览限制：单段列表的冗余容器使派生节点超过通用 UI 配额。Markdown 0.17.0 压缩该布局结构，保留全文、源码映射和既有安全配额，实际 ZIP 原生末尾滚动回归通过，详见[配额修复验收](verification/21-platform-specification-preview-quota.md)。


用户打开 Markdown 文件时，原有源码语法高亮消失，编辑器缺少格式工具栏、实时预览以及两侧同步阅读能力。编写包含图片、表格和任务列表的文档时，需要手动输入标记并切换到外部工具检查效果。

已核实的历史原因：提交 `2018e97b18f70fc6b17be6e4ee31a016ca14d3f7` 将语言识别迁移至插件贡献，移除了 Markdown 等扩展名的宿主回退映射；访谈时仓库未提供 Markdown 语言插件，无提供者时返回纯文本。这是迁移后的支持缺口。用于悬浮提示和插件说明的 Markdown 渲染能力仍存在，不等于文件编辑区已有 Markdown 插件或分栏预览。本方案通过独立插件补齐该缺口，不恢复宿主回退。

## Solution

提供随编辑器发行的独立 Markdown 插件：在受信任工作区首次打开 Markdown 时复用已有权限确认，确认后当前文件直接获得源码高亮、格式工具栏和实时预览，并可管理插入的图片。插件通过公开能力接入，允许禁用、卸载和替换；默认提供不覆盖拒绝或替代提供者选择。

默认采用左侧源码、右侧预览的可调分栏。编辑区顶部放置格式工具栏；底部栏左侧的现有工具按钮组右边，以短竖线分隔，依次放置仅编辑、分栏、仅预览按钮，再放同步滚动开关。三个模式图标参考用户提供的 IDEA 截图语义，按项目现有 SVG 图标风格重新设计。

格式工具栏的十八个按钮均使用 SVG 图标，包括 H1～H6、粗体、斜体、删除线、行内代码、代码块、引用、三种列表、链接、图片和表格。统一 24 × 24 设计网格、2 px 圆角描边，按钮 24 px 高且默认背景透明，窄分栏按组换行。标题在光标当前行或选中行切换级别；重复同级取消标题，不在非空行插入模板或新行，空白行保留本地化占位文字。预览采用 GitHub Primer 深浅配色，行内代码明确继承正文前景。底栏“隐藏/显示Markdown工具栏”图标与三个模式按钮、同步开关同属一组并保持同一间隔，只切换顶部工具栏，不关闭预览，显隐按工作区记忆；非 Markdown 文件不显示整组预览控件。小幅滚动的精确源码提议不能被布局容差吞掉；延迟同步不得撤销手动预览位置。详见[本次验收](verification/15-github-headings-toolbar-scroll.md)与[底栏分组验收](verification/20-footer-toolbar-toggle-group.md)。

## User Stories

1. As an editor user, I want Markdown support bundled with the editor, so that opening a Markdown file in a trusted workspace works immediately.
2. As an editor user, I want Markdown source highlighting, so that headings, emphasis and other markup are easy to distinguish.
3. As an editor user, I want an independently managed Markdown plugin, so that I can disable, uninstall or replace it.
4. As an editor user, I want restricted workspaces to preserve trust boundaries, so that opening a document does not start unauthorized plugins.
5. As a Markdown author, I want common Markdown syntax and GFM tables, task lists and strikethrough, so that ordinary documentation renders consistently.
6. As a Markdown author, I want a formatting toolbar above the source editor, so that I can insert common syntax without memorizing every marker.
7. As a Markdown author, I want heading, bold, italic and strikethrough actions, so that I can structure and emphasize text.
8. As a Markdown author, I want inline code and fenced code block actions, so that I can document code clearly.
9. As a Markdown author, I want quote, unordered list, ordered list and task list actions, so that I can organize content.
10. As a Markdown author, I want link, image and table actions, so that I can insert structured content quickly.
11. As a Markdown author, I want toolbar actions to use my selection, so that existing content can be formatted directly.
12. As a Markdown author, I want templates with editable placeholders when nothing is selected, so that I can continue typing immediately.
13. As a Markdown author, I want one undo step for each formatting action, so that mistakes are easy to reverse.
14. As a Markdown author, I want source on the left and preview on the right, so that I can edit and review together.
15. As a Markdown author, I want a draggable divider, so that I can allocate space to the current task.
16. As a Markdown author, I want the preview to follow unsaved edits, so that I do not need to save to see changes.
17. As a Markdown author, I want edit-only, split and preview-only modes, so that I can choose an appropriate reading or writing layout.
18. As an editor user, I want recognizable mode icons beside the bottom tool group, so that layout controls are easy to find.
19. As an editor user, I want the most recent Markdown mode remembered per workspace, so that switching files preserves my preference.
20. As a Markdown author, I want bidirectional synchronized scrolling by default, so that either pane can guide reading.
21. As a Markdown author, I want scrolling aligned by corresponding content blocks, so that large images and tables do not cause percentage-based drift.
22. As a Markdown author, I want a visible synchronization toggle, so that I can inspect the panes independently when needed.
23. As an editor user, I want the synchronization preference remembered per workspace, so that it survives document switches.
24. As a Markdown author, I want local relative images resolved from the document directory, so that documents remain portable.
25. As a Markdown author, I want authorized network images displayed, so that existing remote image references remain useful.
26. As a Markdown author, I want image failures to show alternative text and a reason, so that I can repair broken references.
27. As a Markdown author, I want pasted and dropped images saved beside the document, so that inserting images needs no manual copy step.
28. As a Markdown author, I want image names allocated as img, img1, img2 with actual format extensions, so that naming is predictable and existing files are preserved.
29. As a Markdown author, I want a save prompt before inserting an image into an unsaved document, so that the image has an unambiguous destination.
30. As a Markdown author, I want task checkboxes in the preview to update source markers, so that I can maintain tasks while reading.
31. As a Markdown author, I want checkbox changes to be undoable, so that preview interaction has the same safety as source editing.
32. As a Markdown reader, I want other preview content to remain read-only, so that browsing does not accidentally rewrite my document.
33. As a Markdown reader, I want fenced code highlighted by enabled language providers, so that code samples are readable.
34. As a Markdown reader, I want plain monospace code when a provider is unavailable, so that missing plugins do not prevent reading.
35. As a Markdown reader, I want heading anchors to navigate within the current document, so that I can follow its structure.
36. As a Markdown reader, I want relative Markdown links to open in the editor, so that documentation browsing stays in my workspace.
37. As a Markdown reader, I want clicked web links to open in the system browser, so that external navigation uses my preferred browser.
38. As an editor user, I want old preview results rejected after editing or switching documents, so that one document never displays another document's content.
39. As an editor user, I want plugin deactivation to remove its UI and subscriptions, so that no empty pane or stale control remains.
40. As an editor user, I want consistent light and dark themes, keyboard focus and Chinese input behavior, so that Markdown editing feels native.
41. As a plugin developer, I want only public versioned capabilities, so that the plugin can be independently built and replaced without host-specific branches.

## Implementation Decisions

### 1. 插件职责与发行

- Markdown 是独立插件包，随发行包提供；受信任工作区默认启用，同时尊重用户明确禁用、卸载与提供者选择，不因默认启用覆盖用户选择。
- 语言识别、Tree-sitter WASM grammar、查询、Markdown 解析、格式命令和预览领域逻辑由插件提供。宿主不得恢复 Markdown 扩展名或插件 ID 的专属业务分支。
- 内置与第三方插件走同一公开能力、权限、实例作用域及生命周期。访客通过宿主 SDK 构建入口独立构建。
- 受限工作区允许普通文本编辑，但不启动插件；默认启用不绕过信任与首次安装权限规则。具体随包权限呈现沿用既有管理流程。

### 2. 语法与预览

- 首版为 CommonMark 基础语法及 GFM 常用扩展：标题、强调、引用、列表、链接、图片、行内代码、围栏代码块、表格、任务列表、删除线。
- 预览使用当前内存文本，跟随输入、粘贴、撤销、重做和磁盘重新加载，不要求先保存。
- 代码块复用已启用语言插件的高亮能力；缺少对应提供者时保留代码块外观与等宽文字。不得引入宿主内建 grammar 作为回退。
- 原生 GPUI 渲染，不使用 WebView 或前端框架。现有宿主 Markdown 视图是可研究的渲染先例，不视为已经向插件公开的完整能力。
- 具体解析库、块映射数据结构和增量渲染策略留给实现设计；不能把未评估的库能力或性能数字写成已确认事实。

### 3. 工具栏与文档写入

- 格式工具栏位于 Markdown 源码编辑区顶部，按用途分组：标题；粗体、斜体、删除线；行内代码、代码块；引用与三种列表；链接、图片、表格。
- 标题按钮覆盖 H1～H6，有选区时切换选中行，无选区时切换光标行；同级取消，不新增行。其他格式按钮有选区时作用于选中文字，无选区时插入模板并选中待填写内容；空白标题行也保留占位模板。每次格式操作是一次可撤销编辑。
- 所有文本写入经过 DocumentSession，EditorState 保持唯一内存文本、选区、IME、布局及 Undo/Redo 真相来源。插件可持有只读快照和派生解析结果，不维护第二份可变文档或撤销栈。
- 预览任务框切换对应源码的 `[ ]`／`[x]`，一次点击对应一次可撤销编辑；其他预览内容只读。
- 通过源文档身份与 revision 校验格式请求和任务点击，拒绝过期操作，避免按过期偏移写入。

### 4. 布局、图标与状态

- 默认左源码右预览，复用 SVG 预览的编辑区分栏交互，允许拖动中间分割线。
- 三种模式：仅编辑、编辑＋预览、仅预览；隐藏的一侧不留空白占位。仅预览时源码工具栏随编辑区隐藏。
- 底部栏左侧现有工具按钮组右侧增加短竖线，然后依次显示三个模式按钮。参考截图的文本行、左右分栏、预览图案语义，生成适合项目的三个 SVG 图标。
- 模式组右侧使用链条图标作为同步滚动开关，包含“同步滚动”提示和明确选中状态；仅分栏模式可操作。
- Markdown 最近视图模式与同步滚动开关按工作区持久化，切换 Markdown 文件沿用；首次默认分栏且同步开启。
- 可见控件沿用 gpui-base 行为与项目本地 UI 外观，支持中英文、深浅主题、焦点、键盘和缩放。停用或移除贡献后收回对应 UI。

### 5. 双向同步滚动

- 用户滚动任一侧时，另一侧按源码与预览的对应段落或内容块定位；不以整篇滚动百分比作为主要映射。
- 同步开关关闭后两侧独立滚动。程序性同步不得再次触发来回滚动循环。
- 映射与源文档 revision 关联，编辑、布局宽度变化和图片加载改变块高度后需更新，不能用旧位置映射污染新文档。
- 首次打开后，延迟的启动配置与同版本布局刷新保留最近手动滚动的一侧，并取消旧定位请求；另一侧仍在顶部的布局通知不得把用户拉回顶部。
- 具体视口通知和定位契约需在实现前核对公开接口；现有画布滚动事件不等价于已具备源编辑器的语义块同步能力。

### 6. 图片读取与插入

- 支持本地相对路径及网络图片；本地路径相对当前 Markdown 文件目录解析，遵守工作区访问边界；网络访问按插件授权执行。
- 图片加载失败时显示替代文字与原因，不使整篇预览失效。
- 拖入或粘贴图片后，保存到当前 Markdown 文件同级目录，插入相对路径引用；不创建 assets 子目录。
- 命名为 `img.实际后缀`、`img1.实际后缀`、`img2.实际后缀` 等，顺序寻找可用名称，保留实际格式对应后缀，禁止覆盖已有文件。扩展名不同不得导致错误声明图像格式。
- 尚未保存的文档先提示保存，无法确定目标目录时不落盘、不插入悬空引用。
- 文件写入受授权与路径校验约束，预览、插图流程不得绕过规范化路径、符号链接和资源归属边界。图片保存失败不得冒充插入成功。
- 文本撤销不删除外部文件：实施阶段在工单 06 明确并验收，Undo／Redo 只恢复相对图片引用，已经保存的图片保留；保存与引用插入分别确认，失败时显示实际文件收据。

### 7. 链接导航

- 点击标题锚点在当前文档中跳转对应位置。
- 点击相对 Markdown 链接，在编辑器内打开目标文档。
- 点击网页链接，在系统浏览器打开；仅解析或预览链接不自动打开浏览器。
- 其他协议和非 Markdown 文件的链接行为未在本轮扩展，沿用现有受控入口，不引入任意 Shell 执行。

### 8. 公开能力与资源生命周期

- 优先复用语言贡献、文档预览订阅、原生 UI、画布、插件管理器和现有文档事务。
- 在实现阶段核对格式写入与选区、编辑区顶部和底部贡献、三态布局、语义滚动、图片访问及代码块高亮等所需接缝。缺失能力以公开、版本化、可撤销的通用接口补齐，不能假定现有协议已全部满足。
- 新能力同步交付权限、请求结果、错误、取消、revision、实例作用域及清理语义，并更新 SDK 和消费者。
- 预览结果须绑定文档身份、revision 和当前实例；文档关闭、切换或贡献撤销后不得应用迟到结果。网络图片任务、滚动订阅、面板和工具栏均随实例撤销。
- 不复制源码编辑器状态，不 fork GPUI Kit，不为本插件建立专用测试宿主入口。

## Testing Decisions

### 已批准主接缝

采用一条主要纵向入口：**真实 Markdown 插件包 → 公开插件管理器的安装／启用／禁用／卸载 → 原生编辑器中的文档与交互 → 可观察结果**。

从用户实际入口验证，不以直接调用插件内部解析函数代替集成验收。复用现有 SVG 内存文档预览和组合 UI 的 GPUI 测试先例，沿现有 Manager 与宿主 worker 发布入口驱动真实包。新公开能力使用独立最小插件验证通用性，不新增 Markdown 专属宿主 API。纯算法测试只补充难以穷举的边界，不形成另一套产品状态真相。

### 验收矩阵

| 编号 | 场景 | 可观察结果 |
| --- | --- | --- |
| M01 | 发行安装、信任与提供者 | 受信任工作区按默认策略获得 Markdown 支持；受限工作区不启动；用户禁用与替代提供者选择被保留 |
| M02 | 常用语法 | 基础语法、表格、任务列表和删除线在源码及预览中符合约定 |
| M03 | 格式工具栏 | 所有按钮覆盖有选区和无选区；插入与选区可观察，单次撤销恢复原文 |
| M04 | 三种布局 | 首次分栏、左右位置正确、分割线可拖动；三个按钮与分隔线位置正确；切换无空白占位 |
| M05 | 偏好持久化 | 工作区内切换文档及重开后保持最近模式与同步开关，不串到另一工作区 |
| M06 | 实时与过期结果 | 未保存输入、粘贴、撤销、重做和重新加载正确反映；旧 revision、关闭和重开后的结果不覆盖当前文档 |
| M07 | 双向滚动 | 两侧均可带动对应块；长图片、表格、换行及宽度变化后保持合理对应；关闭后独立，无反馈循环 |
| M08 | 图片读取 | 相对路径和获授权网络图片可见；缺图、无权限、网络失败时有替代文字与原因 |
| M09 | 插图与命名 | 粘贴、拖入图片保存到同级目录，按 img、img1、img2 分配，保留格式后缀与已有文件；未保存文档先保存 |
| M10 | 写入失败与越界 | 保存失败不产生成功假象；路径越界、符号链接越界、重名竞态不覆盖用户文件 |
| M11 | 任务交互 | 点击仅更新对应任务源码并可撤销；过期预览点击不改错行；其他内容只读 |
| M12 | 代码块高亮 | 已启用语言提供高亮，缺少提供者仍显示等宽代码块，停用提供者正确降级 |
| M13 | 链接 | 标题锚点定位，相对 Markdown 在编辑器打开，网页在点击时通过系统浏览器入口打开 |
| M14 | 生命周期 | 安装热生效；停用、卸载、替换后撤销控件、面板、订阅及未完成任务，无跨实例迟到更新 |
| M15 | 原生体验 | 深浅主题、中英文、中文 IME、键盘焦点、滚动、缩放及窄窗口下布局可用 |
| M16 | 公开契约 | 独立构建与真实包协商通过；新增能力经独立夹具验证，无 Markdown ID 或语言名专属宿主业务分支 |

测试使用隔离目录、临时文档和隔离插件数据。网络响应、系统浏览器调用在既有受控外部边界替换或记录，断言用户可观察结果，不依赖真实外网的稳定性。

实施交付执行仓库规定的针对性测试、格式检查、非 UI workspace 测试、workspace 编译、相关 editor-app 测试和必要原生交互验收；真实 WASM 包先构建再显式执行相应 ignored 测试。Windows 为主要验收平台，不把编译通过当作 UI 验证，不把跳过测试报告为通过。

验收状态以[逐单记录](verification/README.md)为准；只有实际命令、真实包和原生交互满足对应条件后才标记通过。规划阶段未验收的历史状态不作为当前执行结果。

## Out of Scope

- 首版数学公式、Mermaid 和内嵌 HTML 渲染。
- 所见即所得富文本编辑；任务复选框之外的预览直接编辑。
- 新增 Markdown LSP、自动格式化器、PDF／HTML 导出或发布服务。
- WebView、前端框架、宿主内建 Markdown grammar 或插件专属业务分支。
- 全局工具链修改、未经授权网络访问、绕过受限工作区规则。
- 多窗口、自由停靠、协同编辑和其他未经确认的产品扩展。

## Further Notes

### 2026-10-05 已确认的性能与增量要求

超过两屏的 Markdown 即纳入长文档验收，不能只以超大文件为测试样本。输入优先，预览可以滞后 100–200 ms，连续输入期间必须定期更新；仅编辑模式暂停正文生成。普通段落修改局部解析和更新，复用未变化块；涉及列表、围栏、引用、标题锚点等依赖时扩大失效范围，语义正确优先。预览仅排版可见区域与缓冲区，保留图片、链接和远处导航。同步以最后实际用户操作为准，滚轮一格及亚像素位移不能被迟到回执反向覆盖。实施与实测见[增量验收记录](verification/17-incremental-input-viewport.md)。0.15.0 用户仍复现输入卡顿，性能验收保持打开；后续宿主修复与测量边界见[源码输入与过期视口通知](verification/18-source-input-and-stale-viewport.md)。

- 图片存放位置以用户最后修正为准：Markdown 文件同级目录，不能沿用早先建议的 assets 子目录。
- 截图用于解释三个模式图标的语义，不要求复制 IDEA 的像素或外观。最终图标及底栏需服从本地主题与已有工具组布局。
- 规格不会把历史插件平台的“已完成”状态继承为本功能完成。后续实施工单与验收记录须单独建立并同步索引。
- 本方案与插件管理／运行日志改造并存，底栏集成需保留其现有功能，避免覆盖其他任务的修改。
- 实施阶段形成的公开 API、错误、工作预算和图片格式见[公开协议](../../../crates/plugin-protocol/README.md)、[使用说明](../README.md)与逐单验收记录；这些细节必须与真实包和 SDK 一致，不仅停留在方案中。
- 方案与工单已发布并读回原生依赖。用户随后明确要求“开始执行所有工单”，授权按依赖顺序实现、测试与独立审查、普通提交、推送 origin 并核对关闭对应工单；此前仅整理文档的范围限制由本次执行授权取代。父方案、强制推送、历史重写和 GitHub Release 不在该授权内。
