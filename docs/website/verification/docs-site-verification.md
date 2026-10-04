# 文档站点终验记录（E09）

日期：2026-10-04。对应工单：[GitHub #47](https://github.com/T-miracle/Editor/issues/47)。分支：`codex/docs-site`。
本记录只写实际执行过的检查与真实结果；未验证项在末尾单独列出。

## 交付范围

| 栏目 | 页面（每语言） | 内容来源 |
| --- | --- | --- |
| 编辑器使用 | 6 页 | 本次新写（快速开始、编辑与快捷键、文件树、面板与布局、设置与主题、安装与管理插件） |
| 插件系统对接 | 10 页 | 9 篇能力协议从 `crates/plugin-protocol/` 迁入英文树；5 篇中文原生文档逐字复用为中文页，4 篇英文原生文档译为中文 |
| 站点入口 | 首页 + 栏目页 + 搜索页 | 本次新写 |

构建产物 36 页（18 页 × 2 语言），搜索索引覆盖全部 36 页。

## 验收项与结果

| 验收项 | 结果 | 证据 |
| --- | --- | --- |
| 真实 URL 可访问，中英两版两条线页面齐全，无空页与占位页 | 通过 | 站点 `https://t-miracle.github.io/Editor/`（`build_type=workflow`）。部署流水线运行 [#2](https://github.com/T-miracle/Editor/actions/runs/37186100061)：`Build and check` 与 `Deploy to GitHub Pages` 均 success。线上核验：`/Editor/en/`、`/Editor/zh-cn/`、`/Editor/zh-cn/sdk/ui/`、`/Editor/en/search/` 全部 HTTP 200，侧栏 16 项（两栏目各 6/8 项）齐全，中文内容渲染无乱码 |
| 双语页面集合一致、切换入口互指 | 通过 | `tests/doc-sources.test.mjs` 的镜像与互指断言；线上语言切换在 `/Editor/en/sdk/ui/` 与 `/Editor/zh-cn/sdk/ui/` 之间互指 |
| 逐页抽查中英内容语义一致，数值与标识符无偏差 | 通过 | 4 篇中文原生文档与原文按段落多重集比对逐字一致；9 对页面按数字与标识符集合比对，差异仅为写法（`100,000` 与 `100000`、`ui.canvas >=1.1` 与 `>= 1.1`） |
| 站内链接与锚点全部可解析，切换/侧栏/页内/外链正常 | 通过 | `tests/links.test.mjs`（含"不得硬编码 base 前缀"断言）；线上页面源码中的链接均带 `/Editor/` 前缀 |
| 搜索以中英关键词各检索一次命中对应页面 | 通过 | `tests/search.test.mjs`：`fold` 命中 en 树、`编辑` 命中 zh-cn 树；线上搜索页加载 `/Editor/pagefind/pagefind-ui.js` |
| SDK 导出内容与站点英文树一致，验证脚本通过 | 通过 | **导出文档与站点英文页逐字比对 9/9 一致**（剥离 frontmatter 后）；导出 25 个文件且 9 篇以标题开头；`scripts/verify-plugin-sdk.ps1` 退出码 0（含导出、损坏修复、仓库外独立构建组件） |
| 站点不含指向 `docs/`、工单或源码路径的链接 | 通过 | 对 36 个产物 HTML 全文检索 `docs/website`、`docs/plugins`、`docs/agents`、`tickets/`、`verification/`，无命中 |
| 站点协作约定补齐 | 通过 | `website/AGENTS.md`：双语同步规则、目录与内容边界、构建陷阱（搜索索引强制语言、frontmatter 冒号、索引范围、站点目录被宿主引用） |

## 实际执行的命令与结果

```text
cd website
npm test                 → 16 passed / 0 failed / 0 skipped
npm run build            → 36 page(s) built；Indexed 1 language / 36 pages / 3272 words；门禁 16 passed

cargo build -p editor-app
target/debug/editor-app.exe --export-plugin-sdk <dir>   → 退出码 0，25 个文件，文档首行均为标题
./scripts/verify-plugin-sdk.ps1 -HostExe ./target/debug/editor-app.exe → 退出码 0
cargo fmt --check        → 通过
cargo check --workspace  → 通过
```

## 公开面边界

`docs/`（方案、规格、工单、交接）与 `plugins/*/README.md`（插件包内容）均不在站点产物中；
插件包说明由插件管理器在应用内显示，站点只写宿主行为。

## 未验证项与限制

1. **未在原生 GUI 中做交互验收**。编辑器使用指南与插件管理说明的内容来自实现、界面文案与既有
   原生验收记录，未在真实窗口中逐项点击核对。
2. **搜索 UI 的浏览器交互**（下拉、分页、高亮）未在浏览器中点击；已验证索引可被真实检索，
   且线上搜索页可达并正确加载索引资源。
3. **搜索索引为单一强制语言**。Pagefind 索引中文正文需要 CJK 分词扩展，该扩展未发布到 npm；
   不使用 `--force-language` 时中文查询恒返回 0 条。当前配置下中文可检索、英文照常命中，代价是
   没有按语言分片与词干还原。若将来能获取分词扩展，应恢复分片并同步移除本记录与该配置。
4. **`sdk_distribution` 等需要真实 WASM 夹具的 `--ignored` 集成测试未运行**；SDK 链路已由
   `verify-plugin-sdk.ps1` 覆盖（导出、损坏修复、仓库外独立构建组件）。
5. **本地 `main` 未前进**：远端 `main` 已更新到本分支（`f0b6787`），但主工作区存在与本次无关的
   未提交改动，其中 `README.md` 与本次改动冲突，因此未在主工作区执行合并检出，以免覆盖这些改动。
   需要时由维护者在清理工作区后执行 `git fetch origin main:main`（快进）或 `git merge --ff-only`。

