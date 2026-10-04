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
| 真实 URL 可访问，中英两版两条线页面齐全，无空页与占位页 | **部分完成** | 站点已配置为从工作流发布（`https://t-miracle.github.io/Editor/`，`build_type=workflow`）；工作流进入默认分支前无法注册，故线上 URL 未核验（见"未验证项"）。产物侧：36 页、双语各 18 页、无草稿标记残留，`draft` 页不参与构建 |
| 双语页面集合一致、切换入口互指 | 通过 | `tests/doc-sources.test.mjs` 的镜像与互指断言 |
| 逐页抽查中英内容语义一致，数值与标识符无偏差 | 通过 | 4 篇中文原生文档与原文按段落多重集比对逐字一致；9 对页面按数字与标识符集合比对，差异仅为写法（`100,000` 与 `100000`、`ui.canvas >=1.1` 与 `>= 1.1`） |
| 站内链接与锚点全部可解析，切换/侧栏/页内/外链正常 | 通过 | `tests/links.test.mjs`（含"不得硬编码 base 前缀"断言）；外链只计数不断言 |
| 搜索以中英关键词各检索一次命中对应页面 | 通过 | `tests/search.test.mjs`：`fold` 命中 en 树、`编辑` 命中 zh-cn 树 |
| SDK 导出内容与站点英文树一致，验证脚本通过 | 通过 | `scripts/verify-plugin-sdk.ps1` 退出码 0；导出 25 个文件，9 篇文档以标题开头（站点 frontmatter 已在导出时剥离） |
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

1. **线上 URL 未核验**。GitHub Actions 只为默认分支上的工作流建立注册表：工作流文件仅存在于
   `codex/docs-site` 时，`GET /actions/workflows` 返回 `total_count: 0`，推送事件不触发，
   `workflow_dispatch` 也无法发起。因此部署与真实子路径下的 404 检查必须在本分支合并进 `main`
   之后进行。合并后应执行：

   ```powershell
   gh run list --workflow docs-site.yml --limit 1
   curl -sS -o /dev/null -w "%{http_code}\n" https://t-miracle.github.io/Editor/
   curl -sS -o /dev/null -w "%{http_code}\n" https://t-miracle.github.io/Editor/zh-cn/
   ```

2. **未在原生 GUI 中做交互验收**。编辑器使用指南与插件管理说明的内容来自实现、界面文案与既有
   原生验收记录，未在真实窗口中逐项点击核对。
3. **搜索 UI 的浏览器交互**（下拉、分页、高亮）未在浏览器中点击；已验证索引可被真实检索。
4. **搜索索引为单一强制语言**。Pagefind 索引中文正文需要 CJK 分词扩展，该扩展未发布到 npm；
   不使用 `--force-language` 时中文查询恒返回 0 条。当前配置下中文可检索、英文照常命中，代价是
   没有按语言分片与词干还原。若将来能获取分词扩展，应恢复分片并同步移除本记录与该配置。
5. **`sdk_distribution` 等需要真实 WASM 夹具的 `--ignored` 集成测试未运行**；SDK 链路已由
   `verify-plugin-sdk.ps1` 覆盖（导出、损坏修复、仓库外独立构建组件）。
