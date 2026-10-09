# 插件系统文档目录

状态：本轮“插件平台 API 通用化与可扩展性重构”已完成（2026-10-03）。20 张实施工单 / GitHub #2–#21 均已完成实现、验证、独立双轴审查、提交、推送与关闭。最终提交为 `92cca291735a7940990307cea6b9b498d5a46433`；父设计基线 #1 保留。

Windows 原生验收已通过；macOS/Linux 未实测或交叉构建。七个正式插件包已在本地构建和验收，未发布 GitHub Release。完整结果以[最终契约验收](../../verification/plugin-api-contract-verification.md)为准。

## 方案、工单与使用文档

新增方案：[Markdown 插件总方案](../../../../plugins/markdown/docs/spec.md)、[工单目录](../../../../plugins/markdown/docs/tickets/README.md)、[AI 执行入口](../../../../plugins/markdown/AGENTS.md)（产品需求已逐项确认，拆分与测试接缝待确认；尚未发布议题或实施）。按用户明确要求，本批文档放在插件目录内，覆盖原生编辑与预览、格式工具栏、三种视图、双向同步滚动及图片交互。

新增批次：[插件管理与运行日志改造方案](specs/plugin-management-logs.md)（已确认并发布为 [#22](https://github.com/T-miracle/Editor/issues/22)），对应[3 张实施工单](tickets/plugin-management-logs/README.md)。#23 分栏与固定顶部已完成，见[分栏验证](verification/plugin-management-tabs-verification.md)；#24 完整日志与未读图标、#25 底栏 A 方案待实施。本批次独立于下表已完成的平台重构。

| 分类 | 入口 | 状态 |
| --- | --- | --- |
| 平台设计 | [插件平台方案](../../specs/plugin-api-platform.md) | 已完成实施，正文保留设计基线 |
| 实施工单 | [20 张工单、议题映射及依赖图](../../tickets/README.md) | 全部已完成 |
| 最终验收 | [T01–T26 契约验收](../../verification/plugin-api-contract-verification.md) | 已完成，含真实执行证据与限制 |
| 运行与构建 | [运行时插件平台](../../runtime-plugins.md) | 当前使用参考，随平台维护 |
| 声明式主题 | [主题插件格式](../../主题插件格式.md) | 格式参考，随契约维护 |
| 公开 SDK | [协议与 SDK 文档](../../../../crates/plugin-protocol/README.md) | 当前契约；专题文档保留在源码旁 |

## 分阶段验收记录

以下记录均对应已完成工单。正文保留执行当时的范围、结果和限制；早期“后续工单”“旧协议暂存”等描述是历史阶段说明，最终行为以完整契约验收和当前 SDK 为准。

- [bootstrap](../../verification/plugin-api-bootstrap-verification.md)
- [composable-ui](../../verification/plugin-api-composable-ui-verification.md)
- [contract](../../verification/plugin-api-contract-verification.md)
- [data-migration](../../verification/plugin-api-data-migration-verification.md)
- [dependencies](../../verification/plugin-api-dependencies-verification.md)
- [execution-service](../../verification/plugin-api-execution-service-verification.md)
- [hot-update](../../verification/plugin-api-hot-update-verification.md)
- [installed-upgrade](../../verification/plugin-api-installed-upgrade-verification.md)
- [installers](../../verification/plugin-api-installers-verification.md)
- [language-migration](../../verification/plugin-api-language-migration-verification.md)
- [language](../../verification/plugin-api-language-verification.md)
- [lsp](../../verification/plugin-api-lsp-verification.md)
- [process](../../verification/plugin-api-process-verification.md)
- [recovery](../../verification/plugin-api-recovery-verification.md)
- [requests](../../verification/plugin-api-requests-verification.md)
- [scopes](../../verification/plugin-api-scopes-verification.md)
- [services](../../verification/plugin-api-services-verification.md)
- [settings](../../verification/plugin-api-settings-verification.md)
- [terminal-migration](../../verification/plugin-api-terminal-migration-verification.md)
- [ui-migration](../../verification/plugin-api-ui-migration-verification.md)

## 插件包说明

- [终端](../../../../plugins/terminal/README.md)
- [示例](../../../../plugins/example/README.md)
- [SVG](../../../../plugins/svg/README.md)
- [Rust](../../../../plugins/rust/README.md)
- [TOML](../../../../plugins/toml/README.md)
- [HTML](../../../../plugins/html/README.md)
- [JavaScript](../../../../plugins/javascript/README.md)
- [能力协议验收夹具](../../../../plugins/capability-example/README.md)：仅用于验收，不属于发行插件目录。

## 后续管理约定

插件系统的新方案、扩展、迁移和缺陷工单均归入本目录：`specs/` 保存方案，`tickets/` 保存工单与议题映射，`verification/` 保存验收。使用能区分主题或批次的文件名或子目录，保留本轮 01–20 的历史编号，并更新本索引。

每份方案和工单标明实际状态、对应规格、相关工单和验收入口；完成后同步状态与验收项。新任务不继承本轮“已完成”状态或提交、推送、关闭授权。历史记录不覆盖最终完成状态，也不把未验证的平台或未发布的资产标成完成。

返回[当前插件文档入口](../../README.md)。
