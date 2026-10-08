# Nanobug 文档目录

本目录按主题管理项目文档。方案、工单和验收记录相互链接；历史材料保留原始结论，不作为当前实施状态。

| 分类 | 入口 | 用途与状态 |
| --- | --- | --- |
| 项目基线 | [需求整理](project/需求整理.md)、[开发计划](project/开发计划.md) | 已确认的历史产品基线；具体实施进度以对应主题的验收记录为准 |
| 产品品牌 | [Nanobug 名称、图标与兼容标识](project/branding.md) | 当前品牌入口与保留旧数据、开发和仓库标识的依据 |
| 安装与分发 | [安装包目录](distribution/README.md) | 第一期 Windows 安装器与验收；macOS/Linux 兼容打包入口不验收 |
| 插件系统 | [插件文档目录](plugins/README.md) | 方案、工单、验收、运行与主题格式；本轮 20 张平台工单已完成 |
| 原生 UI | [UI 文档目录](ui/README.md) | 原生 UI 方案与 GPUI 交互调研；快捷键面板已交付，资源管理器文件转移代码与自动化验证已完成，原生验收待补充 |
| 文档站点 | [静态文档站点方案](website/specs/static-docs-site.md)、[规格](website/specs/docs-site.md)、[实施工单](website/tickets/README.md)、[交接说明](website/handoff/static-docs-site-handoff.md) | 已发布为 GitHub [#38](https://github.com/T-miracle/Editor/issues/38) 与工单 #39–#47，待实施；读者文档面在 `website/` |
| 协作约定 | [领域术语](agents/domain.md)、[议题工作流](agents/issue-tracker.md)、[分诊标签](agents/triage-labels.md) | AI 与维护者的协作依据；仓库入口为 [AGENTS.md](../AGENTS.md) |

## 后续存放约定

- 插件系统文档统一放在 `plugins/`：方案进 `specs/`，工单进 `tickets/`，执行证据进 `verification/`，使用及格式说明放在主题目录根部。更新[插件索引](plugins/README.md)。
- 其他主题沿用同样的“主题/specs、tickets、verification”结构，按需创建目录，并在本总目录登记；UI 调研放在 `ui/research/`。
- 会话交接按“主题/handoff/”放置，只记录状态、边界与下一步，用路径引用方案正文而不复制其内容；方案或验收更新后同步核对交接是否需要作废。
- 项目级需求和计划放在 `project/`，协作规则放在 `agents/`。失效且没有追溯价值的副本、临时会话资料和源码快照直接移除，历史可从 Git 查询；需求基线和真实验收证据保留并标明适用范围。
- SDK 协议文档：面向读者的能力协议正文按[静态文档站点方案](website/specs/static-docs-site.md)迁往 `website/en|zh-CN/sdk/`，`crates/plugin-protocol/` 只保留指针；插件包说明保留在各自 `plugins/<包名>/README.md`（由插件管理器读取显示），从主题索引链接，避免维护副本。
- 本目录服务于 AI 与维护者，不发布为文档站点。面向使用者的使用说明与插件对接文档放在 `website/`，自成读者工作面；两端是同一仓库的两个视图，不互相复制正文。
- 新方案与工单按实际阶段标注“草案、已确认、待实施、进行中、已完成”；完成时补齐验收证据、实际交付状态和平台限制。历史基线与未验收内容不统一标成已完成。
- 2026-10-08 辅助脚本移出仓库后，历史验收中的 `scripts/` 命令仅保留当时证据，不是当前执行入口；打包步骤以[原生安装器说明](../installer/README.md)为准。
