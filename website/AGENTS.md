# 文档站点 AI 协作约定

本文件是 `website/` 的入口约定，补充仓库根 `AGENTS.md`。站点是**面向读者的工作面**，
与 `docs/`（AI 与维护者工作面）职责分离：读者文档只在 `website/` 手写，`docs/` 的内容
不发布、不在站点引用。

## 双语同步规则（硬性）

- **英文（`src/content/docs/en/`）是权威正本**；中文（`src/content/docs/zh-cn/`）是译文。
  协议或行为表述变更时先改英文，再同步中文。
- **两个语言树必须在同一次提交内更新。** 不允许"先改英文、中文以后补"。
- 每个页面通过 frontmatter 的 `alternate` 声明对侧页面；`tests/doc-sources.test.mjs`
  断言两树页面集合一致且互指，缺页或指错都会让构建失败。
- 测试只能保证**结构对应**，不能保证**语义等价**。涉及数值上限、错误码、权限名称、
  版本号的改动必须逐条对照另一语言复核，不能只让测试通过。
- 中文页面不得比英文更强或更弱地断言同一件事；发现冲突时改中文，不改英文。

## 目录与内容边界

| 路径 | 内容 |
| --- | --- |
| `src/content/docs/<locale>/guide/` | 编辑器使用（面向使用者） |
| `src/content/docs/<locale>/sdk/` | 插件能力协议（面向插件作者） |
| `src/content/docs/<locale>/index.md` | 站点首页，两条读者动线的入口 |
| `src/lib/` | 导航模型与 Markdown 插件 |
| `src/layouts/`、`src/styles/` | 版式与主题，与编辑器主题取向一致 |
| `tests/` | 文档源与链接门禁 |

- 不收录插件包自己的用法（在各自 `plugins/<包>/README.md`，由插件管理器显示）。
- 不收录仓库内部资料：工单、验收记录、协作约定、源码路径与提交哈希都不上站。
- 不收录源码构建、测试与打包命令；那些属于仓库根 README。
- 不发布占位空页：新章节要么有实质内容，要么不建页面。

## 构建约定与陷阱

- Node 版本由仓库根 `.node-version` 固定，CI 工作流显式写同一版本；改一处必须改另一处。
- **Markdown 内的站内链接写站点根路径**（如 `/en/guide/`），不要写平台前缀；
  `src/lib/remark-base-links.mjs` 在构建时注入 `base`，本地开发与线上因此一致。
- `astro.config.mjs` 的 `site` 与 `base` 由同一个仓库名变量推导；仓库改名时两处一起变，
  否则线上所有内部链接 404。
- **站点目录位置被 Rust 侧引用**：编辑器通过 `include_bytes!` 把英文 SDK 文档编进
  可执行文件并导出给插件项目。移动或改名 `src/content/docs/en/sdk/` 会直接导致宿主
  编译失败，这不是意外而是有意的耦合；确需移动时同步修改宿主侧引用并重跑 SDK 验证。

## 验证

```powershell
cd website
pnpm install          # 首次
pnpm test             # 文档源与链接门禁
pnpm build            # 构建静态站点后再次运行同一组测试
pnpm dev              # 本地预览，逐页核对中英两版
```

改动页面后至少运行 `pnpm test`；改版式或链接生成逻辑后运行 `pnpm build` 并核对产物。
纯内容改动不要求运行 Rust 侧检查，但触及站点目录位置或 SDK 文档时按上方陷阱一节处理。
