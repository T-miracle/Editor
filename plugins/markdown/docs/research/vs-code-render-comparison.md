# VS Code Markdown 渲染机制 vs 本项目原生预览：差异对照

日期：2026-10-05。目的：以一手来源对照两者的**预览渲染、更新策略、源码映射、编辑器高亮**四条主线，找出本项目机制的差距与可借鉴点。VS Code 侧事实来自 `microsoft/vscode` @ `48573218e0a08469a8da454b5c005472239425e1`（1.141.0，2026-10-05）的源码，链接为该 commit 的 permalink；本项目侧来自本仓库现行契约与实现。

配套调研：[VS Code Markdown 机制一手来源笔记](vscode-markdown-mechanism.md)（含 40 个源码文件、官方文档与 issue/PR 的逐条出处，以及"未找到一手来源"的显式标注）。

## 一、VS Code 的机制

1. **渲染载体是 WebView + markdown-it，不是原生控件。** 内置扩展 `markdown-language-features` 在扩展宿主里用 `markdown-it`（CommonMark 预设）把 Markdown 渲染成 **一整段 HTML 字符串**，交给 webview 面板显示；`html: true` 允许原文内嵌 HTML，代码块由 `highlight.js` 在渲染时产出高亮 HTML（[markdownEngine.ts](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/markdownEngine.ts)）。
2. **行级源码映射写在 HTML 属性里。** 引擎注册 `pluginSourceMap`：凡是有 `map` 的 token（inline 除外）都加 `data-line="<块起始行>"`、`class="code-line"`、`dir="auto"`；`html_block` 另插一个带属性的空 `<div>` 作标记，文档末尾再补 `data-line=<行数>` 哨兵（同上、[documentRenderer.ts](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/documentRenderer.ts)）。预览→编辑器的点击（`didClick{line}`）与滚动同步都以这个**块起始行**为坐标；反向定位是**像素→行线性插值**，会得到小数行号，并非字符级映射（[scroll-sync.ts](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/preview-src/scroll-sync.ts)）。
3. **更新是 300 ms 节流 + 版本去重。** `#delay = 300`；`refresh()` 首次立即更新，其后合并到一次 `setTimeout`；`#updatePreview` 里若文档版本未变则直接跳过（[preview.ts](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/preview.ts)）。
4. **可见与不可见走两条更新路径。** `shouldReloadPage = forceUpdate || 无版本 || 资源变了 || !webviewPanel.visible`：面板不可见时整页重设 `webview.html`；可见时只 `postMessage({type:'updateContent', content: html, …})`（同上）。也就是说**不可见不省渲染，只是换了注入方式**；每 300 ms 仍会重跑一次 markdown-it 渲染。经典预览不用 `retainContextWhenHidden`，隐藏期间发出的 `updateContent` 增量结果会丢失（官方 webview 文档；相关 open PR #334681）。
5. **真正做增量的是 webview 端的 DOM 差分。** 预览脚本引入 `morphdom`，把新 HTML 与现有 DOM 做结构比对后就地改节点（比较属性时**故意忽略 `data-line` 的差异**），并保留等价子树（如 `<details open>`）；内嵌 `<script>` 由 `domEval` 按保留属性（含 `nonce`）重新执行（[preview-src/index.ts](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/preview-src/index.ts)）。初始内容经 `data-initial-md-content` + `DOMParser` 注入。调研未发现"预览增量解析/增量渲染"的官方 PR、issue 或文档——**服务端始终整篇重渲染**。
6. **token 级缓存**：`TokenCache` 按 (uri, version, breaks, linkify) 复用 token，版本不变时不重复 tokenize（同 markdownEngine.ts）。
7. **滚动同步是行号握手 + 双向自锁。** 预览侧用 `data-line` 反查行号（`getEditorLineNumberForPageOffset`、`getLineElementForFragment`、`scrollToRevealSourceLine`，带 lodash throttle），宿主侧 `scrollEditorToLine`/`TopmostLineMonitor`；两侧各有节流与 100–200 ms 自锁避免互相驱动（preview.ts、scroll-sync.ts）。图片尺寸会改变映射，所以定位前用 `doAfterImagesLoaded` 等图片 load 完成，webview 还会 `cacheImageSizes` 回传尺寸稳定布局（preview-src/index.ts）。
8. **图片与安全走 webview 资源模型**：图片 `src` 重写为 `asWebviewUri` 并保留 `data-src`，对每个本地图片建文件监视器触发刷新；`localResourceRoots` 限定可读目录；`enableScripts: true, enableForms: false`；原始 HTML **没有 sanitizer**，安全依赖 CSP + nonce + 按工作区选择的安全级别（Strict / 允许不安全内容 / 允许脚本等），见 [security.ts](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/security.ts)。
9. **能力面**：内嵌 HTML 允许；KaTeX 由内置扩展 [markdown-math](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-math/package.json) 贡献（`markdownItPlugins` + katex CSS），Mermaid 自 1.121 起由内置 `mermaid-markdown-features` 提供（[release notes](https://code.visualstudio.com/updates/v1_121#_mermaid-diagrams-in-markdown-preview-and-notebooks)）；**经典预览不支持脚注与 GFM 任务列表**，复选框回写只存在于新的 `vscode.markdown.editor` 富编辑器（micromark GFM 路线）。
10. **编辑器高亮走 TextMate grammar（不是 Monarch）+ `injectTo` 注入**（`markdown-basics` 扩展的 `markdown.tmLanguage.json`）。分词在 Web Worker 内按硬编码预算分片：10 ms 防抖、每批 ≤200 行、累计 20 ms 让出；`maxTokenizationLineLength` 默认 20000（超长行不分词），`largeFileOptimizations` 下 >20 MB 或 >300K 行的文件**永不分词**（详见配套笔记第 3 节）。

## 二、本项目的机制

1. **渲染载体是原生声明式 UI 树，没有 WebView、CSS 或脚本。** Markdown 由独立 WASM 访客用 `pulldown-cmark` 解析，产出 `plugin_protocol::ui::Document`（最多 2048 节点 / 2 MiB），宿主用 GPUI 原生控件绘制；富文本是受限 HTML 子集交给原生 `TextView`（[UI.md](../../../../website/src/content/docs/en/sdk/ui.md) 的“只读富文本与源码块映射”）。原文内嵌 HTML 不执行，按文字显示（[README](../../README.md)）。
2. **源码映射是字节范围 + 语义块，不是行号属性。** 每个节点可带 `source_range`（绑定 `Document.source` 版本的半开 UTF-8 区间）；同步滚动不使用行号，而是“顶部源码偏移 + 行内比例”与“预览块 + 块内比例”双向定位，并区分程序回执（`origin`）与手动输入的所有权（[VIEWPORT.md](../../../../crates/plugin-protocol/VIEWPORT.md)）。粒度可细于一行。
3. **更新是“访客发布新树 + 宿主按 revision 门禁”。** 宿主把未保存文本发给访客，访客整份或增量发布；宿主先校验能力、source、预算，再原子替换。现行工作区已加入三条与 VS Code 同向的策略：`ui.incremental` 的 `view_patches` 增量树、`PreviewVisibility{visible}`（仅编辑模式让访客暂停正文解析）、以及首次编辑后 **150 ms 固定节拍**合并正文通知（UI.md 第 11–13 节）。
4. **视口外块只保留测量高度**：`ui/plugin/virtual_blocks.rs` 复用块的已测原生高度并加一屏 overscan，避免整篇重建富文本布局。
5. **代码块高亮**由宿主按公开能力调用已启用提供者的 WASM grammar，带取消与期限，结果绑定源码身份与场景；**源码编辑器高亮**用同一套插件 grammar（`markdown` + 注入 `markdown_inline`）。
6. **能力面**：CommonMark + GFM 表格/任务列表/删除线；任务框可写回源码；图片走受控读取（工作区/网络权限、配额、失败局部降级）；数学公式、Mermaid、内嵌 HTML 渲染明确不支持（[spec.md](../spec.md) 非目标）。

## 三、逐项对照

| 维度 | VS Code | 本项目 |
| --- | --- | --- |
| 预览渲染 | WebView + DOM/CSS，markdown-it 输出 HTML 字符串 | GPUI 原生控件 + 声明式节点树，无 WebView/CSS/脚本 |
| 富文本表达 | 任意 HTML + CSS | 受限 HTML 子集（原生富文本控件解析），原始 HTML 不执行 |
| 源码映射 | 块级 `data-line`（行号） | 节点级 `source_range`（UTF-8 字节）+ 块内比例 |
| 预览→源码定位 | 行号 `revealLine`/`didClick` | 语义块定位请求（`ViewportTarget::Source{offset, line_fraction}`） |
| 同步反馈防护 | `#isScrolling` + 200 ms 计时器 | 程序回执 `origin` 标记 + 手动所有权 + 手势期间定位返回 Cancelled |
| 更新节流 | 300 ms 防抖（首次立即） | 150 ms 固定节拍 + 一次在途发布 + 访客回答后补发最新 |
| 增量方式 | 宿主每次重发整段 body HTML，**webview 端 morphdom 做 DOM 差分**；不可见时整页重载 | 宿主发布结构化树；`ui.incremental` 复用子树 + 视口外块高度复用，无客户端 diff 层 |
| 版本去重 | 文档 version 相同则跳过 | `DocumentVersion` + `preview_version`/能力代次门禁，过期结果拒绝 |
| 解析位置 | 扩展宿主进程（JS，32 位/主线程外） | 同进程 WASM 沙箱线程（wasmtime） |
| 代码块高亮 | 渲染时 highlight.js（同一线程） | 宿主后台批处理 + 已选 WASM 提供者，可取消/超时/降级 |
| 编辑器语法高亮 | TextMate grammar + `injectTo` 注入，worker 内 10 ms 防抖 / ≤200 行每批 / 20 ms 让出；超长行与大文件直接不分词 | 插件 WASM Tree-sitter grammar + 注入查询；当前实现每次解析重建注入 grammar（无编译缓存） |
| 图片 | `asWebviewUri` + `localResourceRoots` + 文件监视 + `cacheImageSizes` 回传尺寸 | 公开能力声明 + 权限 + 配额 + 受控解码，失败局部降级 |
| 任务列表/脚注 | 经典预览不支持 GFM 任务列表与脚注；复选框回写只在新的 `vscode.markdown.editor` 富编辑器 | 原生任务框可写回源码并支持撤销，删除线/表格支持 |
| 内置能力面 | 内嵌 HTML、KaTeX（`markdown-math`）、Mermaid（1.121 起内置）、任意 CSS 主题 | 明确不做内嵌 HTML/数学/Mermaid；主题由宿主角色统一 |
| 安全模型 | CSP + nonce + 按工作区安全级别；**无 HTML sanitizer**；脚本在 webview 内启用 | 无脚本/无 WebView；能力协商 + 权限 + 实例/工作区归属 |
| 大文档 | 浏览器 DOM 承担布局（无节点上限）；>20 MB / >300K 行连语法高亮都关闭 | 2048 节点 / 2 MiB 预算，超出走受限回退 |
| 隐藏时 | 照常每 300 ms 重渲染，改为整页替换；隐藏期间的增量结果会丢 | `PreviewVisibility` 通知访客暂停派生正文解析，重新显示时按最新文本补齐 |

## 四、关键差异与影响

1. **“谁负责增量”不同。** VS Code 每次都可以重发一整段 HTML，因为差分由 webview 端的 morphdom 与浏览器排版承担，节点数不受限；本项目发布的是**结构化树**，宿主必须自己承担增量与虚拟化（`view_patches` + 高度复用），否则每次输入都要重建整棵原生树。这是原生渲染路线必须付的代价：VS Code 的“服务端全量 + 客户端 diff”，对应我们的“服务端增量 + 客户端复用高度”。
2. **映射粒度与同步质量不同。** VS Code 用行号，同步精度受“一行 → 一个块”限制；本项目用字节范围 + 块内比例，可以做到亚行对齐（例如同一段内的图片、表格行），代价是契约复杂（`origin`、手动所有权、revision 校验）。
3. **输入路径的优先级不同。** VS Code 的高亮是 TextMate 分词 + worker 分片预算（10 ms 防抖 / ≤200 行每批 / 20 ms 让出），超长行与大文件干脆不分词，所以输入几乎不受高亮和预览影响；预览又整篇重渲染 + 300 ms 节流，且结果落在另一个渲染进程。本项目的解析在 WASM 线程（不阻塞 UI），但**宿主侧曾有两处落在输入路径上**：源码编辑器的 WASM grammar 装载与每次输入后工具栏/预览的重投影。前者已按本文档的结论修复——每次解析新建的 WASM store 都会重新编译 grammar（release 实测约 93 ms/次输入），现改为共享带编译缓存的引擎后降至 2–7 ms；机制上正对应 VS Code 用“常驻分词器 + 预算”回避的那一类开销。测量装置与数据见 [验证记录 19](../verification/19-source-input-grammar-cache.md)。
4. **能力面各有取舍，不是单方面落后。** 内嵌 HTML、数学公式、Mermaid 在本项目里是有意不做（无 WebView/无脚本/统一主题），代价是能力面窄；反过来 VS Code 的经典预览**不支持 GFM 任务列表与脚注**，复选框回写要换用新的 `vscode.markdown.editor` 富编辑器，而本项目的原生任务框已可写回源码并支持撤销、表格与删除线也已支持。
5. **隐藏时的行为不同。** VS Code 隐藏时仍然每 300 ms 重渲染（只改成整页注入），隐藏期间的增量结果会丢；本项目正在把"仅编辑模式"变成**真正暂停派生正文解析**（`PreviewVisibility`），比 VS Code 更省，但需要保证重新显示时用最新文本补齐（VS Code 的整页重载正好是它的兜底）。

## 五、可借鉴与不应照搬

可以借鉴：

- **“输入优先”的显式分层**：把编译/解析类工作从输入路径移出（缓存编译产物、把首次解析交给后台），并在协议层保留“只是可见性变化”的低成本通知——本项目已有 `PreviewVisibility` 与 150 ms 节拍，差的是 grammar 编译缓存。
- **行级 `data-line` 的简化价值**：我们的块内比例更精确，但在“整块重排/滚动映射”上可以借用其简单模型做快速路径（块首行直接命中，不必走完整定位搜索）。
- **不可见时的显式降级路径**：VS Code 至少保证不可见时不会因为增量状态不一致而显示错内容（整页重载）。我们的“仅显示上一棵树 + 到点补发”需要维持同样的正确性保证。

不应照搬：

- **整篇 HTML 重发 + 浏览器 diff**：我们没有 DOM，重发结构化树必须自己算增量，否则等于把 VS Code 的浏览器成本搬到宿主 UI 线程。
- **允许内嵌 HTML/脚本**：与本项目“无 WebView、无脚本、能力协商”的安全模型直接冲突。
- **行号作为唯一坐标**：会丢掉我们已有的亚行精度与双向手动所有权语义。

## 来源

- VS Code（均为 commit `48573218e0a08469a8da454b5c005472239425e1` 的 permalink）：[preview/preview.ts](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/preview.ts)、[src/markdownEngine.ts](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/markdownEngine.ts)、[src/preview/documentRenderer.ts](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/documentRenderer.ts)、[src/preview/security.ts](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/security.ts)、[preview-src/index.ts](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/preview-src/index.ts)、[preview-src/scroll-sync.ts](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/preview-src/scroll-sync.ts)、[src/vs/workbench/services/textMate/](https://github.com/microsoft/vscode/tree/48573218e0a08469a8da454b5c005472239425e1/src/vs/workbench/services/textMate)（worker 分词预算）、[extensions/markdown-basics/package.json](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-basics/package.json)（TextMate 语法与 `injectTo` 注入）。完整清单见配套笔记"来源"节。
- 本项目：[crates/plugin-protocol/UI.md](../../../../website/src/content/docs/en/sdk/ui.md)、[crates/plugin-protocol/VIEWPORT.md](../../../../crates/plugin-protocol/VIEWPORT.md)、[plugins/markdown/docs/spec.md](../spec.md)、[plugins/markdown/README.md](../../README.md)、[crates/editor-app/src/ui/plugin/virtual_blocks.rs](../../../../crates/editor-app/src/ui/plugin/virtual_blocks.rs)、[crates/editor-app/src/language/plugins.rs](../../../../crates/editor-app/src/language/plugins.rs)。
