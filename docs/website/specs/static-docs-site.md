# 编辑器文档站点方案

状态：已确认。方案与规格已发布为 GitHub [#38](https://github.com/T-miracle/Editor/issues/38)，实施工单 [#39–#47](https://github.com/T-miracle/Editor/issues/39) 已建立原生阻塞边，待实施（尚未建站点目录）。工单拆分见[工单索引](../tickets/README.md)与[规格正文](docs-site.md)。

本文记录"面向读者的静态文档站点"的目录归属、技术选型与维护规则。全部决策经逐项追问确认，取舍理由一并保留，便于后续回看为什么这样定。

## 背景与问题

仓库的 `docs/` 是 **AI/agent 的工作面**（项目基线、插件系统方案与工单、验收记录、协作约定），不是面向读者的产品文档。而现有的面向读者内容分散在三处：

| 位置 | 内容 | 现状 |
| --- | --- | --- |
| `crates/plugin-protocol/*.md` | 插件能力协议契约 9 篇 | 随 SDK 分发，未公开成站点 |
| `plugins/<包>/README.md` | 插件包自己的说明 | 由插件管理器读取并显示；属于插件包内容 |
| 根 `README.md` | 运行、构建、功能清单 | GitHub 门面 |

使用者需要的"编辑器怎么用"目前无处可查；插件作者需要的"怎么对接"只能读源码树。本方案建立独立的读者文档面并发布为静态站点。

## 决策记录

| 主题 | 决策 | 理由与代价 |
| --- | --- | --- |
| 文档面划分 | `docs/` 是 agent 工作面；新建 `website/` 是读者工作面。两者是同一仓库的两个视图 | 仓库保留单一源；`docs/` 整树不进站点，不需要逐文件打排除表 |
| 站点范围 | 只发布"编辑器使用"与"插件系统对接"（能力协议） | 内部工单、验收记录、协作约定不进公开面 |
| 插件包 README | 不进站点 | `management.rs` 从已安装包读取它并在管理器 Overview 显示，生命周期属于插件包，不属于站点 |
| 插件使用说明 | 站点写一页**宿主行为**（安装、权限确认、启停更新卸载、受限工作区限制） | 这是宿主行为，不是插件包内容；不复述各插件自己的用法，避免与包 README 形成第二份源 |
| SDK 文档位置 | 9 篇迁入 `website/en/sdk/`，成为唯一源 | 站点内容集中；`include_bytes!` 与 SDK 摘要缓存机制不受影响 |
| 技术选型 | Astro 自建管线 | 参照 [gpui-kit](https://github.com/longbridge/gpui-kit)，其站点位于主仓库 `website/` 目录 |
| 双语 | `en/` 与 `zh-CN/` 平行，首次即双语 | 英文是权威正本，中文是译文 |
| 权威正本 | 英文为权威；协议变更先改 `en/`，中文同步；SDK 导出取英文版 | 英文已是四篇的原生语言，且是面向第三方作者的公开契约 |
| 防漂移 | 双语页必须同提交；`tests/doc-sources` 断言两个语言树的页面集合一致，缺页即构建失败 | 机制约束，不靠人工记忆 |
| AI 同步规则 | `website/AGENTS.md` 明确要求 AI 修改协议内容时同步更新中英两份 | 用户明确要求 |
| 工程范围 | Pagefind 搜索 + `tests/links`、`tests/doc-sources` + `remarkCallouts`、`remarkSnippets` 两个插件 | 不做 MathJax、不做交互式演示、不做版本化 |
| 部署 | GitHub Pages 项目站 `https://t-miracle.github.io/Editor/`，Astro `base: '/Editor/'` | 零成本且与仓库同名；仓库改名需同步改 `base` 与 `site` |
| 清单格式 | 本次不含"插件清单与资源格式"章节，列为待办 | 该正本在 `crates/plugin-schema/` 的 Rust 类型中，需要独立盘点 |

被否掉的选项与代价：mdBook（中文搜索与提示框能力不足）、VitePress（与所选 Astro 结构相比少一层可控性）、自写 Rust 生成器（长期维护成本最高）、站点只做中文（后续补双语的改造成本远高于现在）、SDK 文档留在 `crates/` 由站点跨目录读取（站点内容源分散）、`plugins/*/README.md` 整体迁入站点（会把插件包内容与宿主文档混在一起）。

## 方案、规格、工单与交接的归档位置

本主题在 `docs/` 下按"主题/specs、tickets、handoff"归档，`website/` 只放站点自身的构建说明，不承载方案与工单：

```text
docs/website/
├─ specs/static-docs-site.md              本方案（决策、结构、改动与验证）
├─ specs/docs-site.md                     按 to-spec 模板整理的规格（问题、故事、实现与测试决策）
├─ tickets/README.md                      实施工单索引、依赖图与实施前沿
├─ tickets/E01..E09-*.md                  九张实施工单（本地制品，尚未发布 GitHub）
├─ tickets/publication.json               发布草稿与稳定标识映射（number/id/url 待发布后回填）
└─ handoff/static-docs-site-handoff.md    会话交接（状态、边界与下一步，引用本文而不复述）
```

## 目标结构

```text
website/                                 ← 新建，读者工作面唯一手写源
├─ AGENTS.md                               AI 协作约定与双语同步强制规则
├─ README.md                               站点自身的构建与部署说明
├─ astro.config.mjs                        site、base、集成、remark 管线
├─ package.json                            scripts: dev / build / preview / test:*
├─ tsconfig.json
├─ src/
│   ├─ layouts/                            版式（顶部导航、语言切换、侧栏、目录）
│   ├─ components/                         页头、侧栏、版本提示等
│   ├─ lib/
│   │   ├─ remark-callouts.js              提示框语法
│   │   └─ remark-snippets.js              从源码树引入片段，避免手抄快捷键
│   └─ styles/                             主题样式
├─ tests/
│   ├─ links.test.ts                       站内链接、锚点与外部链接可达性
│   └─ doc-sources.test.ts                 页面集合、必填 frontmatter、双语页面对应
├─ public/                                 图标、插图、CNAME 无关资源
├─ en/                                     英文（权威正本）
│   ├─ index.md
│   ├─ guide/                              getting-started、editor-and-shortcuts、
│   │                                      explorer-and-search、panels-and-layout、
│   │                                      settings-and-themes、plugins-usage
│   └─ sdk/                                index、capabilities、ui、services、
│                                          processes、languages、lsp、dependencies、
│                                          migration、faults
└─ zh-CN/                                  中文（译文，页面集合必须与 en/ 一致）
    ├─ index.md
    ├─ guide/                              与 en/guide 一一对应
    └─ sdk/                                与 en/sdk 一一对应
```

不再单独建 `scripts/` 下的站点脚本：构建入口是 `website/package.json` 的 `scripts`，部署由 GitHub Actions 调用它们，避免同一件事有两个入口。

## 内容来源与迁移

`website/en/sdk/` 的 10 个页面来自 `crates/plugin-protocol/` 的 9 个 Markdown 文件：前 9 页一一对应，`capabilities.md` 承接 `README.md` 中协议与能力协商部分，`build-and-publish.md` 承接独立构建与 SDK 分发部分（也可合并为 `index.md` 的两节，实施时按篇幅定）。

现状语言分布（实测 CJK 字符数）决定了本次的工作量：

| 语言 | 文件 | 说明 |
| --- | --- | --- |
| 英文 | `DEPENDENCIES.md`、`MIGRATION.md`、`LANGUAGES.md`、`FAULTS.md` | 正文已是英文，可直接成为 `en/sdk/` 对应页 |
| 中文 | `README.md`、`UI.md`、`PROCESSES.md`、`SERVICES.md`、`LSP.md` | 需产出英文权威版，中文原文转为 `zh-CN/sdk/` 译文 |

因此"双语"不是从零翻译：英文侧需把 5 篇中文件转为权威英文，中文侧需为 4 篇英文件补译文。翻译必须保留字节数、条目上限、错误码与版本号等精确表述，不得改写语义。

`crates/plugin-protocol/README.md` 迁移后只保留一行指针（指向 `website/en/sdk/`），说明该 crate 的文档已移至站点、SDK 导出内容取自站点源。

## 需要改动的文件

| 文件 | 改动 |
| --- | --- |
| `website/**` | 新建（结构见上） |
| `website/AGENTS.md` | 新建；双语同提交规则、权威正本、`base` 与链接规则 |
| `crates/editor-app/src/sdk_export.rs` | `SDK_FILES` 中 9 处路径由 `../../plugin-protocol/*.md` 改为 `../../website/en/sdk/*.md`；`SDK_FILES` 的头部注释说明文档源在站点目录 |
| `crates/plugin-protocol/*.md` | 9 篇移出（Git 记录移动），`README.md` 留下指针 |
| `scripts/verify-plugin-sdk.ps1` | 无需改逻辑；核对第 43 行文件存在性断言在迁移后仍通过 |
| `crates/plugin-runtime/tests/sdk_distribution.rs` | 不改；`README.md` 断言针对 `plugins/capability-example/README.md` 夹具，与本次无关 |
| `docs/README.md` | 分类表新增"文档站点"一行；存放约定新增站点与 `docs/` 的职责分界，并修订现第 17 行关于 SDK 文档留在 `crates/` 的表述 |
| `AGENTS.md` | "项目结构与职责"表新增 `website/`；"开始工作与依据"补充读者文档面的入口 |
| `README.md` | 增加一句：使用者文档以站点为准，本文件用于开发者 |
| `.gitignore` | 追加 `website/node_modules/`、`website/dist/`、`.astro/`，并注明构建产物为可再生产物故不入库 |

## 双语防漂移机制

三层约束，缺一不可：

1. **规则**：`website/AGENTS.md` 规定英文为权威正本；改动任何协议表述必须在同一次提交内同步另一语言，禁止只改一边。
2. **测试**：`tests/doc-sources.test.ts` 断言 `en/` 与 `zh-CN/` 的页面路径集合完全一致（缺页、多页都失败），并校验每页 frontmatter 含指向对侧页面的切换链接；`tests/links.test.ts` 校验站内链接与锚点。
3. **CI**：GitHub Actions 在构建前运行上述测试，失败即不部署，避免漂移版本上线。

## 构建与部署

- 本地：`website/` 下执行构建与预览，中文与英文路由、Pagefind 搜索均需人工核对。
- CI：新增 `.github/workflows/`，一条流水线执行安装依赖、`test:links`、`test:docs`、`astro build`（含 Pagefind 索引），产物发布到 GitHub Pages。
- 前置条件：仓库 Settings → Pages 的 Source 需设为 GitHub Actions。
- `astro.config.mjs` 中 `site: 'https://t-miracle.github.io'`、`base: '/Editor/'`，并加注释说明仓库改名时两处需同步修改。

## 验证计划

1. 站点：`website/` 下构建成功，本地预览逐页打开中英两版，搜索命中插件 SDK 关键词，无控制台报错。
2. 站点测试：`tests/links`、`tests/doc-sources` 通过。
3. Rust 侧（本次触及 `sdk_export.rs`）：`cargo fmt --check`、`cargo test --workspace --exclude editor-app`、`cargo check --workspace`。
4. SDK 分发链路：先 `cargo build -p editor-app`，再 `./scripts/verify-plugin-sdk.ps1`；确认导出的 SDK 目录内含英文文档，且摘要缓存路径按新内容重新计算。
5. 迁移一致性：对照检查 9 篇内容在迁移中无丢失段落，相对链接在站点内可解析。

## 待办与遗留

- **插件清单与资源格式**：`manifest.json`、`plugin.toml`、`icons.json`、`plugin-schema` 字段参考尚未纳入站点。正本在 `crates/plugin-schema/` 的 Rust 类型与各插件的 JSON 中，需要先盘点再决定章节划分。
- `docs/plugins/主题插件格式.md`（106 行，中文）经全仓库检索确认未被编入任何产物、也非插件包内容；它是清单格式主题的现成材料，归属应在上述待办中一并决定。
- **版本化**：当前站点只发布最新文档，页内保留 `protocol = 7`、`api.base ^1` 等版本信息，不建 `/versions/` 目录。若将来需要按编辑器版本发布历史文档，参照 gpui-kit 的多次构建方案（`PUBLIC_SITE_BASE` + 独立输出目录 + 链接重写）。
- 快捷键与功能清单在撰写 `guide/` 时必须逐项对照当前实现核对，不照抄根 `README.md` 的既有描述。

## 风险与边界

- 仓库首次引入 Node 与 npm 生态（本项目此前无任何 JS 工程），`package.json`、Astro 与自定义 remark 插件构成新的维护面。
- `include_bytes!` 指向 `website/en/sdk/` 后，移动或改名站点目录会直接导致编译失败；这是有意的耦合，需在 `website/AGENTS.md` 中警告。
- GitHub Pages 项目站的 `base: '/Editor/'` 使所有站内链接必须带该前缀，漏配会导致线上 404。
- 根 `README.md` 的功能清单会与站点内容存在表述重复，以站点为准，但不强制删除 README 中的清单。
