# 插件文档入口

本目录索引实际维护的插件资料。跨插件平台方案放在本目录的 specs、tickets 与 verification；插件自身的历史专题资料保留在对应包内，不复制正文或继承其他任务的完成状态。

- [插件 UI 解耦与文件显示布局总方案](specs/plugin-ui-decoupling.md)、[五个实施工单（#62–#66）](tickets/plugin-ui-decoupling/README.md)。执行分支为 `codex/plugin-ui-decoupling`，验收完成后逐单更新状态。
- [工单 01：Image 与文件显示验收](verification/plugin-ui-decoupling/01-image-file-views.md)。
- [工单 02：可组合布局与提供者选择验收](verification/plugin-ui-decoupling/02-composable-layouts.md)。
- [工单 03：两组底栏与共享偏好验收](verification/plugin-ui-decoupling/03-tools-and-preferences.md)。

- [Markdown 使用说明](../../plugins/markdown/README.md)、[总方案](../../plugins/markdown/docs/spec.md)、[执行工单](../../plugins/markdown/docs/tickets/README.md)、[逐单验收](../../plugins/markdown/docs/verification/README.md)、[AI 执行入口](../../plugins/markdown/AGENTS.md)。
- [插件平台规格](../specs/plugin-api-platform.md)、[平台实施工单](../specs/plugin-api-tickets/README.md)、[运行与构建](../runtime-plugins.md)、[公开协议与 SDK](../../crates/plugin-protocol/README.md)。
- [插件管理与日志方案](specs/plugin-management-logs.md)、[对应工单](tickets/plugin-management-logs/README.md)、[管理页验收](verification/plugin-management-tabs-verification.md)。

平台历史资料当前仍跟踪在 `docs/specs/`；后续重归档应同步修改本入口及消费者链接，不能依赖其他聊天尚未提交的副本。
