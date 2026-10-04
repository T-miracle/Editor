# 交接：编辑器文档站点（方案已定，实施待开始）

状态：方案已确认、实施未开始，尚未发布议题，尚未建 `website/` 目录。
交接对象：接手本任务的全新 agent。生成于方案定稿后的会话收尾。

本文件只说明**状态、边界与下一步**；全部决策、目录结构、迁移映射、改动清单、防漂移机制与验证计划见[静态文档站点方案](../specs/static-docs-site.md)，不在此重复。

## 任务的由来

用户最初的问题是："agent 的目标执行文档都放在 `docs` 目录里，后续要写编辑器使用文档和接口文档、整理成静态网站部署，该放在哪里？"
该问题经逐项追问已完全收敛：`docs/` 是 AI 与维护者的工作面，读者文档另建 `website/`，两者是同一仓库的两个视图。用户随后选定的关键分歧点（双语、权威正本、Astro 自建、GitHub Pages 项目站等）均记入方案文档的决策记录表。

## 先读什么

| 顺序 | 路径 | 作用 |
| --- | --- | --- |
| 1 | [../specs/static-docs-site.md](../specs/static-docs-site.md) | 本任务唯一方案来源：决策记录、目标结构、迁移映射、改动清单、验证计划、待办、风险 |
| 2 | [../../README.md](../../README.md) | 文档分类与存放约定；含"文档站点"入口与 `docs/`↔`website/` 职责分界 |
| 3 | [../../../AGENTS.md](../../../AGENTS.md) | 仓库级协作约定；含 `website/` 职责行与两个工作面的关系 |
| 4 | `crates/editor-app/src/sdk_export.rs` | 实施关键接缝：`SDK_FILES` 内 9 处 Markdown 路径需改指站点目录 |
| 5 | `scripts/verify-plugin-sdk.ps1` | 迁移后必须回归的 SDK 分发验证入口 |

外部参照（同为主仓库内 `website/` + Astro 自建管线）：[gpui-kit 的 website 目录](https://github.com/longbridge/gpui-kit/tree/main/website)。

## 当前工作区状态

`git status --short` 中的改动**混合了历史搬移与本次任务产物**，实施前必须区分：

- 本次任务产物：新增本文件与方案文档，修改 `docs/README.md`、`AGENTS.md`。
- 本次任务之外的既有未提交改动：旧 `docs/specs/**`、`docs/需求整理.md` 等被删除，新的 `docs/plugins/**`、`docs/project/`、`docs/ui/` 未跟踪；另有 `plugins/terminal/README.md`、`scripts/build-plugins.ps1` 的修改。

不要把既有改动当作本任务的成果，也不要在实施时顺手整理它们。提交策略遵循仓库约定：默认不自行提交或推送。

## 下一步

1. 规格与工单**已发布到 GitHub**：方案 [#38](https://github.com/T-miracle/Editor/issues/38)，工单 [#39–#47](https://github.com/T-miracle/Editor/issues/39)，全部带 `ready-for-agent`，14 条原生阻塞边已建立并读回核对；映射见 [publication.json](../tickets/publication.json)。本地索引见[工单索引](../tickets/README.md)。
2. **实施仍被两个外部条件阻塞**：Node 工具链未安装（无法构建与预览站点），GitHub Pages 未启用（`has_pages=false`，需用户在仓库设置中把来源切到 GitHub Actions）。两者都需要用户处理，代理不得擅自安装工具或假定已启用。
3. 解除阻塞后按工单顺序推进，每单的验收条件、阻塞项与验证方式见各自文件；E01 是唯一共同前置。
4. 实施中必须守住的两条硬约束：
   - 双语页面必须同一次提交完成；`en/` 与 `zh-CN/` 的页面集合由测试断言一致，用户明确要求把"AI 同步更新"写成规则。
   - `include_bytes!` 改指站点英文树后，移动或改名站点目录会直接导致编译失败；这是有意耦合，需在 `website/AGENTS.md` 中警告。

## 发布过程记录（供后续批次复用）

- `gh` 未预装；已从官方 release 下载并校验 SHA-256 后解压到仓库外的临时目录使用，未改动系统环境。
- 认证复用了仓库已有的 Git 凭据（管理员权限、`repo` scope）；该令牌缺少 `read:org`，因此 `gh auth login` 会拒绝，改用 `GH_TOKEN` 环境变量即可正常调用 API。令牌未写入仓库。
- 该环境下 PowerShell 5.1 与 git/curl 的 Schannel 调用曾因 `SEC_E_NO_CREDENTIALS` 失败，而 `gh` 与随附 Python（OpenSSL）均可出网；遇到 TLS 失败时优先换用 OpenSSL 栈的工具，而不是判定网络不通。
- 原生依赖边使用 GraphQL `addBlockedBy`，入参为 `issueId`（被阻塞方）与 `blockingIssueId`（阻塞方）的 node id，返回字段为 `issue` 与 `blockingIssue`；不要用 REST 的数字 `id` 代替 node id。

## 未决事项

- 是否现在开始实施；是否先按仓库既有先例拆成实施工单（参考 `docs/plugins/tickets/`）。
- **插件清单与资源格式**章节的归属：正本在 `crates/plugin-schema/` 的 Rust 类型中，需先盘点再定章节划分。
- `docs/plugins/主题插件格式.md` 的去向：经检索确认它未被编入任何产物、也非插件包内容，是上一项待办的现成材料。
- 是否需要按编辑器版本发布历史文档；当前方案只发布最新版，不建 `/versions/`。
- 首次上线前置条件：仓库 Settings → Pages 的 Source 需设为 GitHub Actions（需用户操作）。

## 须向用户确认的假设

- 站点面向**产品使用者**，源码构建、测试与打包内容留在根 `README.md`，不进站点。
- `plugins/<包>/README.md` 属于插件包内容（由插件管理器读取显示），既不进站点，也不迁入 `website/`。
- SDK 文档以**英文为权威正本**、SDK 导出取英文版；需把 5 篇中文转为英文权威版、为 4 篇英文补中文译文，翻译不得改写字节数、条目上限、错误码与版本号等精确表述。
- 部署目标为 GitHub Pages 项目站 `https://t-miracle.github.io/Editor/`，Astro `base: '/Editor/'`；仓库改名需同步修改 `site` 与 `base`。

## 建议技能

| 场景 | 技能 | 理由 |
| --- | --- | --- |
| 开始实施前 | `to-tickets` | 方案已定稿，可拆成带依赖边的实施工单；仓库已有 `docs/plugins/tickets/` 的先例 |
| 直接实施 | `implement` | 按已确认的方案施工 |
| 撰写 `guide/` 内容前 | `research` | 快捷键与功能清单必须对照当前实现核实，不照抄根 `README.md` 的既有描述 |
| 触及 Astro/TypeScript 站点代码 | `documenting-frontend-code`、`organizing-frontend-components` | 仓库首次引入 JS 工程，需遵守注释与目录组织门禁 |
| 方案需再压测 | `grilling` | 本方案即由该技能产出；出现新的未定决策时继续追问 |
| 提交阶段 | `git-commit` | 遵循仓库提交约定 |

## 隐私

本文件不含 API 密钥、口令、令牌或个人身份信息；文中出现的 GitHub 账号与仓库地址均为公开信息。
