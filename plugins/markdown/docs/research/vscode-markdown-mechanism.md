# VS Code 的 Markdown 渲染机制调研（一手来源）

> 调研目标：用微软官方源码与官方文档，逐年逐条回答「Markdown 预览如何渲染、如何更新、编辑器如何高亮、二者如何同步、能力面与性能限制」。
>
> **来源固定（可复现）**：本文所有源码引用都指向 `microsoft/vscode` 的 commit `48573218e0a08469a8da454b5c005472239425e1`（root `package.json` 版本 `1.141.0`，commit 时间 2026-10-05），通过 `git clone --depth 1 --filter=blob:none --sparse` + `git sparse-checkout` 取到 `extensions/markdown-language-features`、`extensions/markdown-basics`、`extensions/markdown-math`、`extensions/mermaid-markdown-features`、`src/vs/editor`、`src/vs/workbench/services/textMate` 等路径。抓取日期：2026-10-05。
> 文中形如 `preview.ts:85` 的行号即该 commit 下的行号；链接均为该 commit 的 permalink。
> 官方文档取自 `microsoft/vscode-docs` 的 `main` 分支源码与 `code.visualstudio.com` 渲染页。
> **术语说明**：本文把 `vscode.markdown.preview.editor` / `markdown.preview` 这个 webview 预览称为「经典预览」，把 `vscode.markdown.editor` 这个新的富文本视图称为「Markdown 编辑器（富编辑器）」。

---

## 0. 结论速览

| 问题 | 结论（一句话） | 关键出处 |
| --- | --- | --- |
| 谁来渲染 | Markdown → HTML 在**扩展宿主（extension host）**里用 markdown-it 全量解析并渲染；DOM 注入、滚动同步、Mermaid/KaTeX 的最终呈现发生在 **webview（渲染进程的沙箱 iframe）** | [markdownEngine.ts:196](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/markdownEngine.ts#L196)、[documentRenderer.ts:111](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/documentRenderer.ts#L111) |
| 用什么库 | `markdown-it@^14.2.0`（CommonMark）+ `highlight.js`；front matter/锚点由扩展自身实现；KaTeX、Mermaid 由两个内置扩展以 `markdownItPlugins` 贡献 | [package.json:1948](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/package.json#L1948)、[markdown-math/package.json](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-math/package.json)、[mermaid-markdown-features/package.json](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/mermaid-markdown-features/package.json) |
| 更新策略 | 预览侧 **300 ms 防抖**（首次立即），文本变化后**整篇重新渲染 HTML**，经 `postMessage('updateContent')` 发送字符串，webview 用 **morphdom 做 DOM 级增量 patch** | [preview.ts:85](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/preview.ts#L85)、[preview.ts:421](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/preview.ts#L421)、[preview-src/index.ts:372](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/preview-src/index.ts#L372) |
| 语法高亮 | 编辑器用 **TextMate grammar**（不是 Monarch），markdown 语法来自内置扩展 `markdown-basics`，其他扩展可 `injectTo: text.html.markdown` 注入 | [markdown-basics/package.json](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-basics/package.json)、[markdown-math/package.json](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-math/package.json) |
| tokenization 预算 | TM 语法在 **web worker** 里分词：10 ms 防抖、每批 ≤200 行、每 20 ms 让出主线程；`editor.maxTokenizationLineLength` 默认 **20000** 字符以上的行不分词 | [textMateWorkerTokenizer.ts:39](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/src/vs/workbench/services/textMate/browser/backgroundTokenization/worker/textMateWorkerTokenizer.ts#L39)、[editorConfigurationSchema.ts:94](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/src/vs/editor/common/config/editorConfigurationSchema.ts#L94) |
| 源码映射粒度 | **块/行**：markdown-it 的 block token 上打 `data-line`（块起始行）+ `class="code-line"`；fenced code block 内部按换行数换算到行 | [markdownEngine.ts:20](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/markdownEngine.ts#L20)、[scroll-sync.ts:59](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/preview-src/scroll-sync.ts#L59) |
| 增量渲染 | **不存在**「增量 Markdown 解析/增量 HTML 生成」；只有 DOM 层的 morphdom 差分与「同版本 token 缓存」 | [markdownEngine.ts:47](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/markdownEngine.ts#L47) |

---

## 1. 预览的进程与组件结构

### 1.1 三个不同的渲染面（都在 `markdown-language-features` 及其兄弟内置扩展里）

| 视图 | viewType | 实现 | 渲染方式 |
| --- | --- | --- | --- |
| 经典预览 / 预览 diff | `markdown.preview`（动态）、`vscode.markdown.preview.editor`（custom editor） | `MarkdownPreviewManager` + `DynamicMarkdownPreview` / `StaticMarkdownPreview` + `MdDocumentRenderer`，webview 侧脚本 `preview-src/**` | markdown-it 生成 HTML 字符串，webview 内 `DOMParser` + morphdom |
| Markdown 编辑器（富编辑器） | `vscode.markdown.editor` | `MarkdownEditorProvider` + 打包的 `markdown-editor-src/editor.ts`，核心是 npm 包 `@vscode/markdown-editor` | 自定义渲染（非 DOM-HTML 往返），并提供 `retainContextWhenHidden: true` |
| Notebook 的 markdown cell | `vscode.markdown-it-renderer` + 各扩展的 renderer 扩展点 | `notebook/index.ts` | markdown-it + `DOMPurify`（仅在 workspace 不受信任时启用） |

出处：[previewManager.ts:76](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/previewManager.ts#L76)（注册 `markdown.preview` serializer 与 `vscode.markdown.preview.editor` custom editor）、[package.json:1898](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/package.json#L1898)（两个 customEditor 及其优先级）、[extension.shared.ts:71](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/extension.shared.ts#L71)（富编辑器注册与 `retainContextWhenHidden`）、[notebook/index.ts:336](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/notebook/index.ts#L336)（notebook renderer 的 DOMPurify 分支）。

### 1.2 进程与线程位置

- **扩展宿主进程**：`MarkdownItEngine`（markdown-it 解析/渲染）、`MdDocumentRenderer`（拼装完整 HTML 文档）、`MarkdownPreviewManager`（面板生命周期、滚动同步）都在扩展宿主里运行——它们只使用 `vscode.*` API，并直接读写 `webview.html` / `webview.postMessage`（[markdownEngine.ts:100](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/markdownEngine.ts#L100)、[documentRenderer.ts:44](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/documentRenderer.ts#L44)、[preview.ts:273](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/preview.ts#L273)）。
- **webview（渲染进程内的沙箱 iframe）**：`preview-src/index.ts` 等脚本负责解析 HTML、morphdom 更新、滚动同步、双击跳转、链接点击转发（[preview-src/index.ts](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/preview-src/index.ts)）。
- **独立的 markdown language server**：链接校验、重命名等语言功能跑在单独的 worker 进程里（Node 下 `TransportKind.ipc` 的 `serverWorkerMain`，Web 下 `new Worker(...)`），与预览渲染不是同一条路径（[extension.ts:38](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/extension.ts#L38)、[extension.browser.ts:28](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/extension.browser.ts#L28)）。

### 1.3 Markdown 库与插件清单

引擎构造顺序（`MarkdownItEngine.#getEngine`）是：`markdown-it` 默认实例 → 关闭 `fuzzyLink` → 依次应用**其他扩展贡献的 `markdownItPlugins`** → 应用内置 front matter 插件 → 依次挂上图片渲染、fenced code 渲染、链接归一化、链接校验、命名锚点、链接渲染、`data-line` 源码映射（[markdownEngine.ts:133](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/markdownEngine.ts#L133)）。

- **markdown-it `^14.2.0`**，配置 `html: true`，并把 fenced code 的高亮委派给 **highlight.js `^11.8.0`**（`hljs` class 由扩展在 fenced 渲染器上加）（[package.json:1948](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/package.json#L1948)、[markdownEngine.ts:398](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/markdownEngine.ts#L398)、[markdownEngine.ts:263](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/markdownEngine.ts#L263)）。
- **front matter**：内置 `yamlPreamble` 插件在 `fence` 规则前注册 `front_matter` block 规则，用 `yaml` 包解析，输出 HTML 由设置 `markdown.preview.frontMatter`（`hide` / `codeBlock` / `table`，默认 `table`）决定（[yamlPreamble.ts:31](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/extensions/yamlPreamble/yamlPreamble.ts#L31)、[previewConfig.ts:50](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/previewConfig.ts#L50)）。
- **锚点（heading id）**：`#addNamedHeaders` 用 slugifier 给标题加 `id`，所以 `#fragment` 链接与「预览内跳转」可用（[markdownEngine.ts:305](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/markdownEngine.ts#L305)）。
- **数学（KaTeX）**：由内置扩展 `markdown-math` 贡献，`markdown.markdownItPlugins: true` + `markdown.previewStyles`（katex CSS）；它在 `extendMarkdownIt` 里 `md.use(require('@vscode/markdown-it-katex').default, options)`，即 **KaTeX 由扩展宿主渲染进 HTML**；设置项 `markdown.math.enabled`（默认 true）与 `markdown.math.macros` 由该扩展声明并触发 `markdown.api.reloadPlugins`（[markdown-math/package.json](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-math/package.json)、[markdown-math/src/extension.ts:17](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-math/src/extension.ts#L17)）。
- **Mermaid**：由内置扩展 `mermaid-markdown-features`（1.121 起内置）贡献；宿主侧只做语法识别——把 ` ```mermaid ` 变成 `<div class="mermaid">`（HTML 转义），**真正的渲染在 webview 里**，由 `markdown.previewScripts` 里的 `./markdown-preview-out/index.js` 执行（[mermaid-markdown-features/src/markdownMermaid/markdownIt.ts:20](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/mermaid-markdown-features/src/markdownMermaid/markdownIt.ts#L20)、[mermaid-markdown-features/package.json:182](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/mermaid-markdown-features/package.json#L182)、[preview-src/markdown/index.ts:50](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/mermaid-markdown-features/preview-src/markdown/index.ts#L50)、[1.121 release notes](https://code.visualstudio.com/updates/v1_121#_mermaid-diagrams-in-markdown-preview-and-notebooks)）。
- **任务列表 / 脚注**：经典预览**没有**对应的 markdown-it 插件（依赖列表里没有任何 task-list / footnote 插件，`media/markdown.css` 里也没有 checkbox 样式），官方 FAQ 明确「不支持 GFM，目标是 CommonMark」（[package.json:1948](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/package.json#L1948)、[markdown.css](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/media/markdown.css)、[官方文档 FAQ](https://code.visualstudio.com/docs/languages/markdown)）。富编辑器是另一条技术路线，见 5.3。

### 1.4 HTML 是怎么生成与注入的

1. 宿主拼装**完整 HTML 文档**：`<meta http-equiv="Content-Security-Policy">`、带 `nonce` 的 `pre.js`、`<base href=...>`、`markdown.previewStyles` 全部样式、`markdown.previewScripts` 全部脚本（带 `nonce`），并把正文 HTML 塞进 `<meta id="vscode-markdown-preview-data" data-initial-md-content="...">` 属性里（[documentRenderer.ts:111](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/documentRenderer.ts#L111)、[documentRenderer.ts:245](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/documentRenderer.ts#L245)）。
2. 正文包装成 `<div class="markdown-body" dir="auto">…</div>`，并在末尾插入哨兵 `<div class="code-line" data-line="lineCount">`，让文档末尾也能参与行映射（[documentRenderer.ts:150](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/documentRenderer.ts#L150)）。
3. webview 载入时从该属性取字符串，`DOMParser.parseFromString(..., 'text/html')` 后 `document.body.append(...)`，再对每个元素执行 `domEval`（把内联 `<script>` 重建为带 `nonce` 的脚本节点，这是贡献脚本/受信任内容执行的路径）（[preview-src/index.ts:88](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/preview-src/index.ts#L88)、[preview-src/index.ts:834](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/preview-src/index.ts#L834)）。
4. 后续更新不再走 `webview.html`，而是 `postMessage({type:'updateContent', content: <body html 字符串>})`，webview 侧解析后 morphdom 合并（见第 2 节）。

### 1.5 CSP / nonce / 清理（sanitize）

- **nonce**：每次 `renderDocument` 生成一个 UUID 作为 nonce，写入 CSP 的 `script-src 'nonce-…'` 与所有 `<script nonce=…>`（[documentRenderer.ts:103](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/documentRenderer.ts#L103)）。
- **CSP 等级**：源码里有四级 `Strict`(默认) / `AllowInsecureLocalContent` / `AllowInsecureContent` / `AllowScriptsAndAllContent`，分别展开成不同的 `img-src`/`media-src`/`script-src`/`style-src`/`font-src` 白名单；`AllowScriptsAndAllContent` 直接返回空 CSP（即完全不限制脚本）。`Strict` 下 `img-src 'self' <cspSource> https: data:`，**http 图片被拦**（[security.ts:10](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/security.ts#L10)、[documentRenderer.ts:257](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/documentRenderer.ts#L257)）。
- **没有 HTML sanitizer**：经典预览不使用 DOMPurify，也不清洗标签属性；依托 `html: true` 直出 + CSP + webview 隔离（[markdownEngine.ts:401](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/markdownEngine.ts#L401)；`DOMPurify` 只出现在 notebook renderer，并且只在 `ctx.workspace.isTrusted === false` 时生效（[notebook/index.ts:339](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/notebook/index.ts#L339)）。
- 曾有一个「纵深防御」PR 提议剥离内联事件处理属性（`onclick`/`onerror`…），但**被关闭且未合并**（`merged: false`，2026-08-23 关闭），当前源码中不存在该函数（[PR #317474](https://github.com/microsoft/vscode/pull/317474)）。
- 预览侧还会拦掉注入内容里的 `meta[http-equiv]`，并把 `<link>` 移到 `<head>` 防白屏（[preview-src/index.ts:349](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/preview-src/index.ts#L349)）。
- CSP 违规会在预览右上角显示可点击的提示条，点击可打开安全等级选择器（[preview-src/csp.ts:20](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/preview-src/csp.ts#L20)、[security.ts:105](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/security.ts#L105)）。

---

## 2. 更新与失效策略

### 2.1 谁是触发源、防抖多少毫秒

- 触发源：`onDidChangeTextDocument`（仅当是当前预览的 uri）、`onDidOpenTextDocument`、文件系统 watcher（外部改动；若该文件已是打开的 TextDocument 则跳过 watcher 以免重复刷新）、贡献变化（`markdownItPlugins`/脚本/样式变化 `refresh(true)`）、设置变化（`updateConfiguration()`）（[preview.ts:153](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/preview.ts#L153)、[previewManager.ts:321](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/previewManager.ts#L321)）。
- **防抖：`readonly #delay = 300`（毫秒）**，源码注释写明「第一次调用立即刷新，紧随其后的调用被 debounce」（[preview.ts:85](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/preview.ts#L85)、[preview.ts:251](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/preview.ts#L251)）。
- 另有**版本短路**：若文档 `version` 与上次渲染相同且非强制，则只重发一次 `updateView`（滚动），不重新渲染（[preview.ts:323](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/preview.ts#L323)）。
- 外部覆盖磁盘的场景见 [PR #300028](https://github.com/microsoft/vscode/pull/300028)（已合并）与仍开放的 [PR #316756](https://github.com/microsoft/vscode/pull/316756)；后者指出「`refresh()` 本身已防抖，所以不会双渲染」。

### 2.2 整篇重发还是增量更新

**两层要分开看**：

1. **Markdown 解析与 HTML 生成是整篇全量的**。每次更新都会对整篇文档调用 `markdown-it` 的 parse + render（`MarkdownItEngine.render`），没有任何按 diff 增量解析；唯一的缓存是 `TokenCache`——仅当「同一 uri + 同一 document.version + 相同 breaks/linkify 配置」时复用 token 数组，编辑一次即失效（[markdownEngine.ts:170](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/markdownEngine.ts#L170)、[markdownEngine.ts:47](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/markdownEngine.ts#L47)）。
2. **DOM 更新是增量的**：webview 收到新 HTML 字符串后，用 **morphdom** 以 `childrenOnly: true` 打补丁；`onBeforeElUpdated` 里用 `areNodesEqual` 判断子树是否等价，等价则整棵跳过（只把 `data-line` 手工搬过去），并保留 `<details>` 的 `open` 状态（[preview-src/index.ts:372](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/preview-src/index.ts#L372)、[preview-src/index.ts:797](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/preview-src/index.ts#L797)）。等价判定明确忽略 `open` 属性与 `data-line` 值差异。

### 2.3 传输通道

| 方向 | 通道 | 载荷 |
| --- | --- | --- |
| 宿主 → webview（整页重建） | `webviewPanel.webview.html = html` | 完整 HTML 文档 |
| 宿主 → webview（常规更新） | `webview.postMessage` | `{type:'updateContent', content: <body html>, lineChanges, diffScrollSync, source}` |
| 宿主 → webview（滚动） | `postMessage` | `{type:'updateView', line, source}` |
| 宿主 → webview（选区高亮） | `postMessage` | `{type:'onDidChangeTextEditorSelection', line, source}` |
| webview → 宿主 | `vscode.postMessage`（`acquireVsCodeApi`） | `revealLine` / `didClick` / `openLink` / `cacheImageSizes` / `showPreviewSecuritySelector` / `previewStyleLoadError` |

出处：[preview.ts:421](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/preview.ts#L421)（`html` 与 `postMessage` 的分支）、[preview.ts:182](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/preview.ts#L182)（收到的消息 switch）、[types/previewMessaging.d.ts:54](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/types/previewMessaging.d.ts#L54)（两个方向的完整消息类型表）。

**何时走整页替换**（`shouldReloadPage`）：强制刷新、首次渲染、预览的资源发生变化（切到另一个 md 文件）、或 **webview 当前不可见**（[preview.ts:331](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/preview.ts#L331)）。

### 2.4 预览不可见 / 被隐藏时是否停止更新

- **不停止触发**：只要文档变化，`refresh()` 照样被调用、300 ms 后照样重新渲染 HTML；不可见只影响「用哪种方式交付」。
- 官方 Webview API 文档写明：面板进入后台标签页后变隐藏但**不会销毁**，重新回到前台时 VS Code 会**用 `webview.html` 恢复内容**；`getState/setState` 才是官方推荐的轻量持久化方式，`retainContextWhenHidden` 有较高内存开销（[官方 Webview API 文档](https://code.visualstudio.com/api/extension-guides/webview)）。
- 这带来一个已知缺陷：可见期间的 `updateContent`（DOM 增量）结果在隐藏→再次显示时会丢失，只剩最后一次全量 `webview.html`。开放中的 [PR #334681](https://github.com/microsoft/vscode/pull/334681) 用「跟踪 `webview.html` 是否落后并在隐藏时强制全量刷新」来修，并顺带指出：**当 300 ms 定时器挂起时，`refresh(true)` 的 force 标志会被静默降级为普通更新**（该 PR 未合并，属一手作者分析，非当前代码事实）。相关用户报告：[#271233](https://github.com/microsoft/vscode/issues/271233)、[#147718](https://github.com/microsoft/vscode/issues/147718)。
- 富编辑器则显式使用 `retainContextWhenHidden: true`（[extension.shared.ts:75](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/extension.shared.ts#L75)）。

### 2.5 滚动位置与选区/光标如何保持

**编辑器 → 预览**
1. 编辑器可视区变化 → `TopmostLineMonitor`（**50 ms 节流**）算出最顶可见行；`getVisibleLine()` 返回**带小数**的行号（按首个可见字符在行内的比例），而不是整数行（[topmostLineMonitor.ts:19](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/topmostLineMonitor.ts#L19)、[topmostLineMonitor.ts:96](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/topmostLineMonitor.ts#L96)）。
2. 宿主向 webview 发 `updateView`；webview 侧再用 `lodash.throttle(..., 50)`，并根据 `scrollPreviewWithEditor` 决定是否滚动（[preview-src/index.ts:157](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/preview-src/index.ts#L157)）。
3. 滚动前会等所有图片 load/error（`doAfterImagesLoaded`），避免布局未稳定时算错位置（[preview-src/index.ts:69](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/preview-src/index.ts#L69)）。

**预览 → 编辑器**
1. webview 滚动事件 50 ms 节流，把像素位置换算成小数行号，发 `revealLine`（同时用 `vscode.setState` 记录 `scrollProgress`）（[preview-src/index.ts:770](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/preview-src/index.ts#L770)）。
2. 宿主 `#onDidScrollPreview` 受 `markdown.preview.scrollEditorWithPreview` 控制，调用 `scrollEditorToLine()`；小数部分会被换算成「行内字符偏移」再 `revealRange(..., AtTop)`（[preview.ts:365](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/preview.ts#L365)、[scrolling.ts:17](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/scrolling.ts#L17)）。

**防回声（feedback loop）**
- 宿主侧用 `#isScrolling` 标志 + 200 ms 定时器，在「预览驱动编辑器滚动」的窗口内拒绝反向的 `scrollTo`（[preview.ts:378](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/preview.ts#L378)）。
- webview 侧用 `scrollDisabledCount` + 50/100/200 ms 的窗口，在程序化滚动期间不上报 `revealLine`（[preview-src/index.ts:17](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/preview-src/index.ts#L17)）。

**选区 / 光标与重载后的位置**
- 光标所在行通过 `onDidChangeTextEditorSelection` → `ActiveLineMarker`，给对应 `code-line` 元素加 `code-active-line` class（左侧灰条），受 `markdown.preview.markEditorSelection` 控制（[previewManager.ts:781](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/previewManager.ts#L781)、[activeLineMarker.ts:11](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/preview-src/activeLineMarker.ts#L11)）。
- 整页重载后，webview 用 `vscode.getState()` 里的 `scrollProgress`（滚动比例）或 `line` / `fragment` 恢复位置；图片尺寸由 `cacheImageSizes` 回传宿主并写成 `.loading` 元素的固定宽高，用于减少重载时的跳动（[preview-src/index.ts:48](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/preview-src/index.ts#L48)、[documentRenderer.ts:217](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/documentRenderer.ts#L217)）。

---

## 3. 编辑器里的 Markdown 语法高亮

### 3.1 tokenizer：TextMate grammar（不是 Monarch）

- markdown 语言由内置扩展 **`markdown-basics`** 提供，`contributes.grammars` 指向 `./syntaxes/markdown.tmLanguage.json`，`scopeName` 为 `text.html.markdown`，并声明了 `embeddedLanguages`（把 fenced code 内的 `meta.embedded.block.python` 等映射到对应语言）与 `unbalancedBracketScopes`（[markdown-basics/package.json](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-basics/package.json)）。
- 语法文件本身由 `microsoft/vscode-markdown-tm-grammar` 转换而来，文件头记录了来源 commit；它自带 `#frontMatter`、`#fenced_code_block_*`、`#table`、`#html` 等规则（[markdown.tmLanguage.json](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-basics/syntaxes/markdown.tmLanguage.json)）。
- **注入（injection）**：其他扩展用 `injectTo: ["text.html.markdown"]` 往 markdown 语法里注入新规则。官方例子就是数学：`markdown-math` 注入了 `markdown.math.block` / `markdown.math.inline` / `markdown.math.codeblock` 三套语法，并把 `meta.embedded.math.markdown` 映射为 `latex`（[markdown-math/package.json](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-math/package.json)）。
- Monarch 在仓库里是另一套独立的 lexer（`src/vs/editor/standalone/common/monarch/monarchLexer.ts`），Monarch 与 TextMate 是并列的两条 tokenization 路线；markdown 走的是 TextMate 路线（由 `contributes.grammars` 声明决定）。Monarch 侧同样实现了 `maxTokenizationLineLength` 检查（[monarchLexer.ts:480](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/src/vs/editor/standalone/common/monarch/monarchLexer.ts#L480)）。

### 3.2 tokenization 的时间预算与后台分片

TextMate 语法由 `TextMateTokenizationFeature` 注册，`TokenizationRegistry` 分发；对每个 model，`ThreadedBackgroundTokenizerFactory.createBackgroundTokenizer` 会建一个 **Web Worker**（`TextMateWorker`）来分词——但前提是 `editor.experimental.asyncTokenization` 为真且 model 没有「大到不适合同步」（`textModel.isTooLargeForSyncing()`）（[threadedBackgroundTokenizerFactory.ts:60](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/src/vs/workbench/services/textMate/browser/backgroundTokenization/threadedBackgroundTokenizerFactory.ts#L60)、[textMateTokenizationFeatureImpl.ts:305](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/src/vs/workbench/services/textMate/browser/textMateTokenizationFeatureImpl.ts#L305)）。

Worker 内的实际预算（全部是硬编码常量，非设置项）：

- **10 ms 防抖**：`new RunOnceScheduler(() => this._tokenize(), 10)`，每次内容变化/失效都重置调度（[textMateWorkerTokenizer.ts:39](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/src/vs/workbench/services/textMate/browser/backgroundTokenization/worker/textMateWorkerTokenizer.ts#L39)）。
- **每批 ≤ 200 行**：`if (lineToTokenize === null || tokenizedLines > 200) break;`（[textMateWorkerTokenizer.ts:137](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/src/vs/workbench/services/textMate/browser/backgroundTokenization/worker/textMateWorkerTokenizer.ts#L137)）。
- **每 20 ms 让出**：累计耗时 > 20 ms 时 `break` 并通过 `setTimeout0(() => this._tokenize())` 让出，好让 worker 处理新的编辑消息（注释直接写着 “yield to check for changes”）（[textMateWorkerTokenizer.ts:156](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/src/vs/workbench/services/textMate/browser/backgroundTokenization/worker/textMateWorkerTokenizer.ts#L156)）。
- **逐行预算（长行豁免）**：`editor.maxTokenizationLineLength` 默认 **20000**，达到或超过该长度的行直接返回 `nullTokenizeEncoded`，即不 tokenize（描述原文：Lines above this length will not be tokenized for performance reasons）（[editorConfigurationSchema.ts:94](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/src/vs/editor/common/config/editorConfigurationSchema.ts#L94)、[tokenizationSupportWithLineLimit.ts:36](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/src/vs/workbench/services/textMate/browser/tokenizationSupport/tokenizationSupportWithLineLimit.ts#L36)）。
- **大文件豁免**：`largeFileOptimizations`（默认开）下，文本 > **20 MB** 或行数 > **300K** 的 model 被判定为 `isTooLargeForTokenization`，**永不 tokenize**（构造时就决定，注释强调 “under no circumstances”）；> **50 MB** 则 `isTooLargeForSyncing()`（[textModel.ts:188](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/src/vs/editor/common/model/textModel.ts#L188)、[textModel.ts:338](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/src/vs/editor/common/model/textModel.ts#L338)）。
- **主线程侧的启发式**：`isCheapToTokenize(lineNumber)` 只在「该行已经是准确状态」或「它正是第一个失效行且长度 < `CHEAP_TOKENIZATION_LENGTH_LIMIT = 2048`」时返回 true，用于决定能否在关键路径上同步补算（[textModelTokens.ts:24](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/src/vs/editor/common/model/textModelTokens.ts#L24)、[textModelTokens.ts:129](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/src/vs/editor/common/model/textModelTokens.ts#L129)）。
- **状态可观察**：`backgroundTokenizationState`（`InProgress` / `Completed`）让 UI 知道「当前 token 可能不准确」；diff 视图等就依赖它在完成后再渲染 token 背景（[tokenizationTextModelPart.ts:96](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/src/vs/editor/common/tokenizationTextModelPart.ts#L96)、[diffEditorViewZones.ts:92](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/src/vs/editor/browser/widget/diffEditor/components/diffEditorViewZones/diffEditorViewZones.ts#L92)）。
- **避免闪烁**：worker 回传 token 时，宿主侧会用 `MonotonousIndexTransformer` 把「在结果产出之后又被编辑过的行」的 token 丢掉，注释写明这是 “to prevent flickering”（[textMateWorkerTokenizerController.ts](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/src/vs/workbench/services/textMate/browser/backgroundTokenization/textMateWorkerTokenizerController.ts)）。

### 3.3 高亮会不会阻塞输入（含 IME 与长文档）

有源码支撑的部分：

- 常规大小文档：TextMate 分词在 worker 里跑，主线程不做全量分词；worker 还会每 20 ms 主动让出，因此**渲染线程不会被 markdown 分词长时间占用**（见 3.2 的三条预算）。
- 极端文档：`> 20000` 字符的单行、`> 20 MB` 或 `> 300K 行` 的文件**根本不分词**（这是「性能优先于高亮」的明确取舍）。
- 仍可能在主线程做同步分词的路径：关闭 `editor.experimental.asyncTokenization`（源码默认 `true`）（[editorConfigurationSchema.ts:99](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/src/vs/editor/common/config/editorConfigurationSchema.ts#L99)）、model 触发 `isTooLargeForSyncing()`（>50 MB）时退回主线程 tokenizer，以及需要 `forceTokenization` 的交互（此时用 `isCheapToTokenize` 的 2048 字符启发式限制成本）。

**未找到一手来源**：我没有找到任何官方文档或源码注释，专门说明「TextMate tokenization 与 IME 组合（composition）事件之间的关系」，也没有找到「输入优先于 tokenization」的明文承诺。因此本文不对「IME 组合期间是否会因高亮卡顿」下断言（详见第 7 节）。

### 3.4 富 Markdown 编辑器（`vscode.markdown.editor`）的高亮是另一套

富编辑器不使用 TextMate，而是把代码块高亮通过 **proposed API `documentSyntaxHighlighting`** 拿到的 token 渲染（`WebviewSyntaxHighlighter`，把颜色映射成 CSS class `tok-mdhl-fg-*` / `tok-mdhl-fs-*`）（[syntaxHighlighter.ts:1](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/markdown-editor-src/syntaxHighlighter.ts#L1)、[editor.ts:11](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/markdown-editor-src/editor.ts#L11)）。

---

## 4. 源码映射与滚动同步

### 4.1 预览元素如何携带源码行信息

- markdown-it 的自定义 core rule `source_map_data_attribute` 遍历所有 block token：`token.map` 存在且不是 `inline` 时，写入 `data-line="<起始行>"`，并追加 class `code-line` 与 `dir="auto"`（[markdownEngine.ts:20](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/markdownEngine.ts#L20)）。
- `html_block` 渲染器不尊重 attributes，扩展专门在外面插一个携带属性的空 `<div>` 作为标记（[markdownEngine.ts:32](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/markdownEngine.ts#L32)）。
- 文档末尾补一个 `data-line="<lineCount>"` 的哨兵元素，保证「最后一块之后」也能定位（[documentRenderer.ts:150](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/documentRenderer.ts#L150)）。
- webview 侧把这些元素收集成 `CodeLineElement[]`（缓存到 `documentVersion` 变化为止），`element` 与 `codeElement` 分离以处理 fenced code block：`<pre>` 拿块起始行，`<code>` 按换行数算出 `endLine`（[scroll-sync.ts:42](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/preview-src/scroll-sync.ts#L42)）。

### 4.2 双方交换的消息

| 消息 | 方向 | 语义 |
| --- | --- | --- |
| `updateContent` | 宿主 → webview | 新正文 HTML（+ 可选 `lineChanges` / `diffScrollSync`） |
| `updateView` | 宿主 → webview | 把预览滚到某一源码行 |
| `onDidChangeTextEditorSelection` | 宿主 → webview | 光标行变化 → 左侧灰条 |
| `copyImage` / `openImage` | 宿主 → webview | 图片上下文菜单动作 |
| `revealLine` | webview → 宿主 | 预览滚动 → 编辑器应显示的行 |
| `didClick` | webview → 宿主 | 双击预览 → 打开编辑器并跳行 |
| `openLink` | webview → 宿主 | 需要宿主解析的相对链接 |
| `cacheImageSizes` | webview → 宿主 | 回传图片尺寸用于稳定布局 |
| `previewStyleLoadError` / `showPreviewSecuritySelector` | webview → 宿主 | 样式加载失败提示 / 打开安全等级选择器 |

出处：[types/previewMessaging.d.ts](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/types/previewMessaging.d.ts)、[preview.ts:182](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/preview.ts#L182)、[preview-src/index.ts:317](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/preview-src/index.ts#L317)。
预览 diff（side-by-side）另开一条 **BroadcastChannel**（`md-diff-scroll-<uuid>`）在左右两个 webview 之间广播行号并做映射，避免绕宿主（[diffScrollSync.ts:19](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/preview-src/diffScrollSync.ts#L19)、[previewManager.ts:270](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/previewManager.ts#L270)）。

### 4.3 映射粒度：块/行，不是字符

- **粒度是「块起始行」**：`data-line` 来自 markdown-it 的 `token.map[0]`，一个段落/列表/表格/代码块只有起始行；只有 fenced code block 通过 `endLine` 获得了行范围内的细分。
- **反向（像素 → 源码行）是插值**：`getEditorLineNumberForPageOffset` 用二分找到最近的 `code-line` 元素，再按「元素内偏移 / 元素高度」线性插值成**小数行号**；代码块内部还会扣掉 padding 按内容高度插值（[scroll-sync.ts:279](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/preview-src/scroll-sync.ts#L279)）。
- 因此这套映射既不是字符级，也不是严格语义级：**同一段落内任意像素位置都会映射成同一段落的插值结果**，`revealLine` 传回宿主的也是整数行（`Math.floor`）（[preview-src/index.ts:777](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/preview-src/index.ts#L777)）。

### 4.4 与任务描述中命名的差异（重要更正）

任务里提到的 `getEditorLineForPreviewPosition`、`didChangeViewZones` 等名字，**在当前源码中不存在**。实际对应关系是：

| 任务描述中的名字 | 当前源码中的真实名字与位置 |
| --- | --- |
| `getEditorLineForPreviewPosition` | `getEditorLineNumberForPageOffset(offset, documentVersion)`（[scroll-sync.ts:279](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/preview-src/scroll-sync.ts#L279)） |
| （预览侧定位元素） | `getElementsForSourceLine` / `getElementsForSourceLineRange` / `getLineElementForFragment`（[scroll-sync.ts:87](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/preview-src/scroll-sync.ts#L87)） |
| `revealLine` | 存在，方向是 **webview → 宿主**（预览滚动驱动编辑器）（[preview.ts:192](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/preview.ts#L192)） |
| `didChangeViewZones` | 不存在；相近的是 webview 面板的 `onDidChangeViewState`（用于维护 active preview）与 `TopmostLineMonitor.onDidChanged`（[previewManager.ts:391](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/previewManager.ts#L391)、[topmostLineMonitor.ts:44](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/topmostLineMonitor.ts#L44)） |

---

## 5. 能力面

### 5.1 原始 HTML

允许。markdown-it 以 `html: true` 运行，markdown 里的原始 HTML 会原样进入预览 DOM（[markdownEngine.ts:401](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/markdownEngine.ts#L401)）。安全性完全交给 CSP 与 webview 隔离：`Strict` 等级下 `script-src 'nonce-<随机 UUID>'`，作者无法猜到 nonce，因此内联脚本不执行；把安全等级调到 **Disable** 时 CSP 为空字符串，脚本即可执行（[documentRenderer.ts:257](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/documentRenderer.ts#L257)）。

### 5.2 数学公式（KaTeX）

支持，由内置扩展 `markdown-math` 提供：`$x^2$` 与 `$$…$$`（以及 fenced math，因为调用时带 `enableFencedBlocks: true`、`globalGroup: true`）；设置 `markdown.math.enabled`（默认 true）与 `markdown.math.macros`；改动设置会触发 `markdown.api.reloadPlugins` 重建引擎（[markdown-math/src/extension.ts:17](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-math/src/extension.ts#L17)、[markdown-math/package.json](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-math/package.json)、[官方文档](https://code.visualstudio.com/docs/languages/markdown)）。注意：**语法着色**（编辑器里）也是这个扩展贡献的注入语法，所以数学在编辑器与预览里都有支持。

### 5.3 Mermaid

支持（1.121 起内置）。链路：宿主把 ` ```mermaid ` 变成 `<div class="mermaid">`（HTML 转义，不做图形渲染）→ webview 脚本用 `mermaid` 库渲染 → 提供平移/缩放/控件/右键「复制源码」「在编辑器中打开」，命令的 `when` 条件明确覆盖 `markdown.preview` 与 `vscode.markdown.preview.editor`（[markdownIt.ts:20](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/mermaid-markdown-features/src/markdownMermaid/markdownIt.ts#L20)、[preview-src/markdown/index.ts:28](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/mermaid-markdown-features/preview-src/markdown/index.ts#L28)、[mermaid-markdown-features/package.json:63](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/mermaid-markdown-features/package.json#L63)、[1.121 release notes](https://code.visualstudio.com/updates/v1_121#_mermaid-diagrams-in-markdown-preview-and-notebooks)）。Mermaid 渲染由自定义事件 `vscode.markdown.updateContent` 触发重渲染，这个事件正是预览在每次内容更新后派发的（[preview-src/index.ts:413](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/preview-src/index.ts#L413)）。

### 5.4 脚注 / 任务列表 / 复选框回写

- **经典预览：不支持**。依赖里没有任何 GFM/footnote/task-list 插件，`media/markdown.css` 里也没有 checkbox 样式；官方 FAQ 明确「不支持 GitHub Flavored Markdown，目标是 CommonMark + markdown-it」（[package.json:1948](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/package.json#L1948)、[markdown.css](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/media/markdown.css)、[官方文档 FAQ](https://code.visualstudio.com/docs/languages/markdown)）。所以**预览里没有「点复选框回写文档」的通路**。
- **富 Markdown 编辑器（`vscode.markdown.editor`）：是 GFM 路线**。它依赖 npm 包 `@vscode/markdown-editor`，而该包依赖 `micromark-extension-gfm`（含 `gfm-footnote`、`gfm-task-list-item`、`gfm-table`、`gfm-strikethrough`）（[package-lock.json:635](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/package-lock.json#L635)），仓库里还带了一个 `test-workspace/checkbox-count-extension` 演示如何消费任务状态（[test-workspace/checkbox-count-extension/src/extension.ts:106](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/test-workspace/checkbox-count-extension/src/extension.ts#L106)）。
- **未能取到一手来源**：复选框点击后如何把编辑写回 `TextDocument`（编辑管线、epoch 校验）实现在 npm 包 `@vscode/markdown-editor` 内部，源码不在本仓库；仓库内只能看到宿主侧的 `RecoveringTaskQueue` epoch 机制为「离线的编辑队列」提供陈旧任务跳过语义（[recoveringTaskQueue.ts:19](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/recoveringTaskQueue.ts#L19)、[markdownEditorProvider.ts:346](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/markdownEditorProvider.ts#L346)）。

### 5.5 图片路径解析

- **webview 资源根**：`localResourceRoots` = 贡献预览脚本/样式的扩展目录 + 工作区所有文件夹（无工作区时取 md 文件所在目录）（[preview.ts:475](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/preview.ts#L475)、[resources.ts:15](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/util/resources.ts#L15)）。
- **改写**：图片渲染器把 `src` 换成 `webview.asWebviewUri(...)` 的结果，原始值留在 `data-src`；`file://` 链接与相对路径（含以 `/` 开头、按工作区根解析的路径）都会被正确改写；无 scheme 的链接用 `markdown-link:` 伪 scheme 解析（[markdownEngine.ts:241](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/markdownEngine.ts#L241)、[markdownEngine.ts:353](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/markdownEngine.ts#L353)）。
- **远程图片与 CSP**：`Strict` 只允许 `https:` 与 `data:`（http 被拦）；`AllowInsecureLocalContent` 额外允许 `http://localhost:*` / `http://127.0.0.1:*`；`AllowInsecureContent` 允许任意 `http:`（[documentRenderer.ts:257](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/documentRenderer.ts#L257)）。
- **本地图片变化会刷新预览**：扩展会对 `containingImages`（渲染时收集到的图片 src）逐个建 `FileSystemWatcher`，变化即 `refresh(true)`；`http/https/data` 不建 watcher（[preview.ts:444](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/preview.ts#L444)）。
- 自定义 CSS：`markdown.styles` 支持 https URL 与工作区相对路径，经 `#fixHref` 解析后注入 `<link class="code-user-style">`（[documentRenderer.ts:198](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/documentRenderer.ts#L198)）。

### 5.6 链接点击与外部打开

- 链接渲染时同时写入 `href` 与 `data-href`（保留作者原样，供宿主解析）（[markdownEngine.ts:335](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/markdownEngine.ts#L335)）。
- webview 的点击处理：`#fragment` 链接直接放行给浏览器内跳转；`http:`/`https:`/`mailto:`/`vscode:`/`vscode-insiders:` **直接放行**（由 webview 宿主处理）；看起来不象 URL 的相对链接才 `postMessage('openLink')` 交回扩展（[preview-src/index.ts:733](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/preview-src/index.ts#L733)）。
- 宿主侧 `MdLinkOpener`：`external` → `vscode.env.openExternal(uri, { allowContributedOpeners: true })`（非 http(s) 则走 `vscode.open`）；`folder` → `revealInExplorer`；`file` → `vscode.open` 并带上 selection/viewColumn（[openDocumentLink.ts:92](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/util/openDocumentLink.ts#L92)、[openDocumentLink.ts:150](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/util/openDocumentLink.ts#L150)）。
- 打开 md 链接的策略由设置 `markdown.preview.openMarkdownLinks` 决定（默认 `inPreview`：在同一个预览里切文件；否则交给 VS Code 打开）（[preview.ts:479](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/preview.ts#L479)）。
- 其他交互：双击预览跳编辑器（`markdown.preview.doubleClickToSwitchToEditor`，对 `.copilotmd` 禁用）（[preview-src/index.ts:709](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/preview-src/index.ts#L709)）；fenced code block 有复制按钮（[preview-src/index.ts:234](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/preview-src/index.ts#L234)）。

### 5.7 预览相关设置一览（源码默认值）

`markdown.preview.breaks=false`、`linkify=true`、`typographer=false`、`frontMatter='table'`、`scrollPreviewWithEditor=true`、`scrollEditorWithPreview=true`、`markEditorSelection=true`、`doubleClickToSwitchToEditor=true`、`fontSize`（≥8）、`lineHeight`（≥0.6）、`fontFamily`、`styles=[]`，以及继承编辑器的 `editor.scrollBeyondLastLine` / `editor.wordWrap`（[previewConfig.ts:34](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/previewConfig.ts#L34)）。

---

## 6. 已知性能策略与限制

### 6.1 预览侧（源码事实）

1. **300 ms 防抖 + 首次即时**：连续输入不会每键渲染（[preview.ts:85](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/preview.ts#L85)）。
2. **版本短路**：version 未变则只重发滚动消息，不重新渲染（[preview.ts:323](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/preview.ts#L323)）。
3. **token 缓存**：仅在「同一文档 + 同 version + 同 breaks/linkify」下复用 markdown-it token（编辑即失效，因此对「边打字边预览」几乎无收益）（[markdownEngine.ts:47](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/markdownEngine.ts#L47)）。
4. **DOM 层差分**：morphdom + `areNodesEqual` 跳过等价子树；`data-line` 变化不算子树变化（这正是「编辑器里插入一行不该整篇重排」的关键）（[preview-src/index.ts:372](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/preview-src/index.ts#L372)）。
5. **图片尺寸稳定化**：webview 回传图片宽高，宿主把它们写成 `#image-N.loading { width/height }`，减少重载/刷新时的跳动与滚动抖动（[documentRenderer.ts:217](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/documentRenderer.ts#L217)）。
6. **多级节流**：webview 滚动上报 50 ms、`updateView` 滚动 50 ms、`TopmostLineMonitor` 50 ms、防回声窗口 50/100/200 ms（[preview-src/index.ts:770](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/preview-src/index.ts#L770)、[preview-src/index.ts:157](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/preview-src/index.ts#L157)、[topmostLineMonitor.ts:19](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/topmostLineMonitor.ts#L19)）。
7. **没有虚拟化/分块**：正文是一次性渲染的一整棵 DOM；也没有按可视区渲染的机制——这是大文档性能的根本限制（`renderBody` 直接整篇 `markdown-body`）（[documentRenderer.ts:135](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/documentRenderer.ts#L135)）。
8. **diff 视图的映射缓存**：`MarkdownPreviewLineDiffProvider` 按 original/modified 的 version 缓存行级 diff 与映射表（[lineDiff.ts:29](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/lineDiff.ts#L29)）。
9. **富编辑器侧**：`RecoveringTaskQueue` 用 epoch 让过期任务直接跳过（不断累积渲染任务），`retainContextWhenHidden: true` 免去隐藏/恢复时的重建成本（代价是内存）（[recoveringTaskQueue.ts:60](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/recoveringTaskQueue.ts#L60)、[extension.shared.ts:75](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/extension.shared.ts#L75)）。

### 6.2 与性能相关的官方 PR / issue（含未合并项，需区分）

| 编号 | 状态 | 与本文的关系 |
| --- | --- | --- |
| [PR #339545](https://github.com/microsoft/vscode/pull/339545)（fixes [#339536](https://github.com/microsoft/vscode/issues/339536)） | **open** | 「markdown: Avoid repeated scans when marking diff lines」：对 944 KB / 16000 段 / 32000 新增行的文档，生产 handler 中位耗时 **921.4 ms → 142.1 ms**（复用源码行扫描、避免重复写 class、清理旧 diff 装饰）。这是目前能看到的最直接的大文档预览性能证据，但**尚未合并** |
| [PR #334681](https://github.com/microsoft/vscode/pull/334681) | **open** | 隐藏面板重建前的强制刷新；顺带暴露「300 ms 定时器挂起时 `refresh(true)` 被降级为非强制」的节流缺陷 |
| [PR #316756](https://github.com/microsoft/vscode/pull/316756) | open | 文档已打开时也响应文件系统变化（修复外部覆盖后预览陈旧，[#265277](https://github.com/microsoft/vscode/issues/265277)） |
| [PR #300028](https://github.com/microsoft/vscode/pull/300028) | 已合并 | 同上问题的早期修复；明确说明「`refresh()` 已防抖，因此不会双渲染」 |
| [PR #317474](https://github.com/microsoft/vscode/pull/317474) | **closed，未合并** | 剥离内联事件处理属性的纵深防御（当前代码里没有） |
| [PR #315119](https://github.com/microsoft/vscode/pull/315119) | 已合并 | diff 预览改用 BroadcastChannel 做左右滚动同步（对应 `diffScrollSync.ts`） |
| [PR #322274](https://github.com/microsoft/vscode/pull/322274) | open | 给 fenced code block 加复制按钮（当前 main 已有该实现） |
| [Issue #293028](https://github.com/microsoft/vscode/issues/293028) | closed | 「Markdown 预览内置 Mermaid」的功能请求；其评论确认 1.121 起内置，并记录了与第三方 mermaid 扩展的冲突 |
| [Issue #301936](https://github.com/microsoft/vscode/issues/301936) | closed | 用户报告的「编辑 markdown 文件很慢」 |

**未找到**：没有任何官方 issue/PR/文档专门讨论「Markdown 预览的增量（incremental）渲染 / 局部重渲染」这一设计议题；搜到的都是 diff 装饰扫描、隐藏面板重建、外部改动刷新这类外围问题。

### 6.3 「输入优先于预览」有没有官方说法

- **未找到**任何官方文档或源码注释以「输入优先 / input takes priority over the preview」之类的措辞做出承诺。能作为间接证据的只有实现层面的事实：预览更新固定 300 ms 防抖且不阻塞编辑器（渲染在扩展宿主，DOM 更新在 webview），以及编辑器侧 tokenization 在 worker 里跑、并有 20 ms/200 行/20000 字符/20 MB 等一整套预算与豁免（见第 2、3 节）。
- 另有一条**相反方向**的官方文档证据：Webview 指南明确说 `retainContextWhenHidden` 内存开销高、应优先用 `getState/setState`，也就是官方在预览类 UI 上更倾向于「重建 + 状态持久化」而不是「常驻保活」（[官方 Webview API 文档](https://code.visualstudio.com/api/extension-guides/webview)）。

---

## 7. 未找到一手来源 / 无法验证的点

1. **IME（输入法组合）与 tokenization 的交互**：未找到任何官方文档、源码注释或 issue 说明「组合期间是否暂停/降级 tokenization」。本文不做断言。
2. **富编辑器的复选框回写实现**：宿主侧看不到「点击复选框 → 写回 TextDocument」的代码路径；实现在 npm 包 `@vscode/markdown-editor`（`0.0.2-107`）内，仓库外，无法用一手源码验证。
3. **增量预览渲染**：未找到任何官方 PR/issue/文档描述「按 diff 增量重渲染 Markdown 预览」；从源码看该机制不存在（只有 DOM 层 morphdom 与整篇 HTML 重发）。
4. **`getEditorLineForPreviewPosition` / `didChangeViewZones`**：这两个名字在当前源码中不存在（见 4.4 的对照表），推测来自其他编辑器（如旧版 VS Code 或别的项目）的命名，本文给出真实替代名。
5. **PR 数据的使用限制**：第 6.2 节中标注 open/closed 的 PR，其正文属于微软一方作者的分析与基准数字，但**不是已合并代码的事实**；本文已逐条标注状态。

## 8. 与官方文档不一致 / 需要留意之处

1. **安全等级数量**：官方文档只列了 `Strict` / `Allow insecure content` / `Disable` 三档（[官方文档](https://code.visualstudio.com/docs/languages/markdown)），源码里实际有**四档**（多一个 `Allow insecure local content`，即允许 localhost 的 http）（[security.ts:10](https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/security.ts#L10)）。
2. **术语漂移**：文档把 Mermaid/KaTeX 都归在「Markdown preview」下；从源码看它们由两个独立内置扩展（`mermaid-markdown-features`、`markdown-math`）贡献，且 Mermaid 的图形渲染发生在 webview 脚本里，宿主侧只做语法识别。另外仓库里还存在一个名字不同、能力更强的新视图 `vscode.markdown.editor`（GFM、任务列表、代码块内嵌编辑器），文档尚未把它与 `vscode.markdown.preview.editor` 区分说明。
3. **文档未覆盖的机制**：文档没有描述 300 ms 防抖、morphdom DOM 差分、`data-line` 映射、以及 tokenization 的具体预算——本文这些结论全部来自源码。

---

## 来源

### 源码（`microsoft/vscode`，pin 到 commit `48573218e0a08469a8da454b5c005472239425e1`，版本 1.141.0）

1. `extensions/markdown-language-features/src/preview/preview.ts` — https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/preview.ts
2. `extensions/markdown-language-features/src/preview/documentRenderer.ts` — https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/documentRenderer.ts
3. `extensions/markdown-language-features/src/preview/previewManager.ts` — https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/previewManager.ts
4. `extensions/markdown-language-features/src/preview/previewConfig.ts` — https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/previewConfig.ts
5. `extensions/markdown-language-features/src/preview/security.ts` — https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/security.ts
6. `extensions/markdown-language-features/src/preview/topmostLineMonitor.ts` — https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/topmostLineMonitor.ts
7. `extensions/markdown-language-features/src/preview/scrolling.ts` — https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/scrolling.ts
8. `extensions/markdown-language-features/src/preview/lineDiff.ts` — https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/lineDiff.ts
9. `extensions/markdown-language-features/src/preview/recoveringTaskQueue.ts` — https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/recoveringTaskQueue.ts
10. `extensions/markdown-language-features/src/preview/markdownEditorProvider.ts` — https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/preview/markdownEditorProvider.ts
11. `extensions/markdown-language-features/src/markdownEngine.ts` — https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/markdownEngine.ts
12. `extensions/markdown-language-features/src/markdownExtensions.ts` — https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/markdownExtensions.ts
13. `extensions/markdown-language-features/src/extension.shared.ts` / `extension.ts` / `extension.browser.ts` — https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/extension.shared.ts
14. `extensions/markdown-language-features/src/util/resources.ts` / `util/openDocumentLink.ts` — https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/util/resources.ts
15. `extensions/markdown-language-features/src/extensions/yamlPreamble/yamlPreamble.ts` — https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/src/extensions/yamlPreamble/yamlPreamble.ts
16. `extensions/markdown-language-features/preview-src/index.ts`、`scroll-sync.ts`、`csp.ts`、`activeLineMarker.ts`、`diffScrollSync.ts`、`settings.ts`、`messaging.ts`、`pre.ts` — https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/preview-src/index.ts
17. `extensions/markdown-language-features/types/previewMessaging.d.ts` — https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/types/previewMessaging.d.ts
18. `extensions/markdown-language-features/package.json`、`package-lock.json`、`media/markdown.css`、`esbuild.webview.mts`、`esbuild.markdownEditor.mts` — https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/package.json
19. `extensions/markdown-language-features/markdown-editor-src/editor.ts`、`syntaxHighlighter.ts` — https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/markdown-editor-src/editor.ts
20. `extensions/markdown-language-features/notebook/index.ts` — https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-language-features/notebook/index.ts
21. `extensions/markdown-basics/package.json`、`syntaxes/markdown.tmLanguage.json` — https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-basics/package.json
22. `extensions/markdown-math/package.json`、`src/extension.ts`、`syntaxes/md-math*.tmLanguage.json` — https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/markdown-math/package.json
23. `extensions/mermaid-markdown-features/package.json`、`src/markdownMermaid/markdownIt.ts`、`preview-src/markdown/index.ts`、`esbuild.webview.mts` — https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/extensions/mermaid-markdown-features/package.json
24. `src/vs/workbench/services/textMate/browser/backgroundTokenization/threadedBackgroundTokenizerFactory.ts` — https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/src/vs/workbench/services/textMate/browser/backgroundTokenization/threadedBackgroundTokenizerFactory.ts
25. `src/vs/workbench/services/textMate/browser/backgroundTokenization/worker/textMateWorkerTokenizer.ts` — https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/src/vs/workbench/services/textMate/browser/backgroundTokenization/worker/textMateWorkerTokenizer.ts
26. `src/vs/workbench/services/textMate/browser/backgroundTokenization/worker/textMateTokenizationWorker.worker.ts` — https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/src/vs/workbench/services/textMate/browser/backgroundTokenization/worker/textMateTokenizationWorker.worker.ts
27. `src/vs/workbench/services/textMate/browser/backgroundTokenization/textMateWorkerTokenizerController.ts` — https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/src/vs/workbench/services/textMate/browser/backgroundTokenization/textMateWorkerTokenizerController.ts
28. `src/vs/workbench/services/textMate/browser/textMateTokenizationFeatureImpl.ts` — https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/src/vs/workbench/services/textMate/browser/textMateTokenizationFeatureImpl.ts
29. `src/vs/workbench/services/textMate/browser/tokenizationSupport/tokenizationSupportWithLineLimit.ts` — https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/src/vs/workbench/services/textMate/browser/tokenizationSupport/tokenizationSupportWithLineLimit.ts
30. `src/vs/editor/common/config/editorConfigurationSchema.ts` — https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/src/vs/editor/common/config/editorConfigurationSchema.ts
31. `src/vs/editor/common/model/textModel.ts` — https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/src/vs/editor/common/model/textModel.ts
32. `src/vs/editor/common/model/textModelTokens.ts` — https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/src/vs/editor/common/model/textModelTokens.ts
33. `src/vs/editor/common/tokenizationTextModelPart.ts` — https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/src/vs/editor/common/tokenizationTextModelPart.ts
34. `src/vs/editor/standalone/common/monarch/monarchLexer.ts` — https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/src/vs/editor/standalone/common/monarch/monarchLexer.ts
35. `src/vs/editor/browser/widget/diffEditor/components/diffEditorViewZones/diffEditorViewZones.ts` — https://github.com/microsoft/vscode/blob/48573218e0a08469a8da454b5c005472239425e1/src/vs/editor/browser/widget/diffEditor/components/diffEditorViewZones/diffEditorViewZones.ts

### 官方文档

36. Markdown and Visual Studio Code（含 Markdown preview / Extending the Markdown preview / Markdown preview security / FAQ）— https://code.visualstudio.com/docs/languages/markdown ；文档源文件：https://github.com/microsoft/vscode-docs/blob/main/docs/languages/markdown.md
37. Webview API（Visibility and Moving / getState and setState / retainContextWhenHidden）— https://code.visualstudio.com/api/extension-guides/webview ；源文件：https://github.com/microsoft/vscode-docs/blob/main/api/extension-guides/webview.md
38. VS Code 1.121 Release Notes — Mermaid diagrams in Markdown preview and Notebooks — https://code.visualstudio.com/updates/v1_121#_mermaid-diagrams-in-markdown-preview-and-notebooks

### 官方 issue / PR

39. #293028 Built-in Mermaid.js support for Markdown previews — https://github.com/microsoft/vscode/issues/293028
40. #339545 markdown: Avoid repeated scans when marking diff lines（open，fixes #339536）— https://github.com/microsoft/vscode/pull/339545 ；https://github.com/microsoft/vscode/issues/339536
41. #334681 Refresh markdown preview html before a hidden panel is rebuilt（open）— https://github.com/microsoft/vscode/pull/334681
42. #316756 Markdown preview: refresh on file-system change even when document is open（open）— https://github.com/microsoft/vscode/pull/316756
43. #300028 Fix markdown preview not refreshing on external file changes（已合并）— https://github.com/microsoft/vscode/pull/300028
44. #317474 defense-in-depth: strip inline event handler attributes from Markdown Preview HTML（closed，未合并）— https://github.com/microsoft/vscode/pull/317474
45. #315119 Use a broadcast channel for md preview diff scroll sync（已合并）— https://github.com/microsoft/vscode/pull/315119
46. #322274 Add copy button to fenced code blocks in Markdown preview（open）— https://github.com/microsoft/vscode/pull/322274
47. #271233 / #147718 预览在隐藏后重新显示时内容陈旧的用户报告 — https://github.com/microsoft/vscode/issues/271233 ；https://github.com/microsoft/vscode/issues/147718
48. #265277 Markdown preview stale after external overwrite — https://github.com/microsoft/vscode/issues/265277
49. #301936 Edit markdown file so slow — https://github.com/microsoft/vscode/issues/301936
