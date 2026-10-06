# 插件文档入口

本目录只索引实际维护的资料。独立插件的方案、工单与验收按用户要求保存在各自包内；不复制正文或继承其他功能的完成状态。

- [Markdown 使用说明](../../plugins/markdown/README.md)、[总方案](../../plugins/markdown/docs/spec.md)、[执行工单](../../plugins/markdown/docs/tickets/README.md)、[逐单验收](../../plugins/markdown/docs/verification/README.md)、[AI 执行入口](../../plugins/markdown/AGENTS.md)。
- [插件平台规格](../specs/plugin-api-platform.md)、[平台实施工单](../specs/plugin-api-tickets/README.md)、[运行与构建](../runtime-plugins.md)、[公开协议与 SDK](../../crates/plugin-protocol/README.md)。
- [插件管理与日志方案](specs/plugin-management-logs.md)、[对应工单](tickets/plugin-management-logs/README.md)、[管理页验收](verification/plugin-management-tabs-verification.md)。
- [运行、调试与构建父议题](https://github.com/T-miracle/Editor/issues/48)、[接手终验与工单处置](verification/run-debug-build-completion-2026-10-06.md)、[B1 UI 修复与复验](verification/run-debug-build-ui-fix-2026-10-06.md)、[历史验收](verification/run-debug-build.md)、[整批初审记录（2026-10-05）](verification/run-debug-build-review-2026-10-05.md)。当前进度以终验及复验记录为准。
- [本批调试器、受控传输与单目标握手决定](specs/run-debug-build-debugger.md)。
- [构建与运行配置 B2：IDEA 参考设计提案](specs/run-config-idea-design.md)（用户已选 A / 方案 1）；[B3：双栏简洁版](specs/run-config-simple-design.md)（已落地原生弹窗，支持插件默认目标）；[B3 实现与验收](verification/run-config-simple-2026-10-06.md)。设计图与真实验证的范围在对应记录中区分。
- [运行配置重构：插件模板、原生表单与配置树](specs/run-config-plugin-tree.md)／[规格 #67](https://github.com/T-miracle/Editor/issues/67)（2026-10-07 产品基线及测试入口已确认）。新规格替代 B3 的表单、目录组织、提交和存储约定，不将旧验收计作新功能通过。
- [运行配置重构实施工单](tickets/run-config-plugin-tree/README.md)：4 个端到端切片 #68–#71 的实现与验收均完成，普通提交及 GitHub 交付记录见各工单；没有合并主分支。30 个验收场景的实际包、Windows 与真实调试证据见 [验收记录](verification/run-config-plugin-tree.md)。

平台历史资料当前仍跟踪在 `docs/specs/`；后续重归档应同步修改本入口及消费者链接，不能依赖其他聊天尚未提交的副本。
