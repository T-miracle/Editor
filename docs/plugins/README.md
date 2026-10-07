# 插件文档入口

本目录索引实际维护的插件资料。跨插件平台方案放在本目录的 specs、tickets 与 verification；插件自身的历史专题资料保留在对应包内，不复制正文或继承其他任务的完成状态。

- [XML 插件与通用语言编辑能力方案](specs/xml-language-tools.md) / [#72](https://github.com/T-miracle/Editor/issues/72)，[三个实施工单 #73–#75](tickets/xml-language-tools/README.md)：全部验收、推送并核对关闭；包含可替换格式化、XML/HTML 标签编辑及宿主大纲与停靠，父设计议题保持不变。
- [XML 语言工单 01 验收记录](verification/xml-language-tools/01-xml-language.md)：记录实际包、原生输入与服务结果，当前状态以记录为准。
- [格式化与标签编辑工单 02 验收记录](verification/xml-language-tools/02-format-and-tags.md)：独立提供者、XML/HTML 配对编辑及原生输入顺序。
- [宿主大纲工单 03 验收记录](verification/xml-language-tools/03-outline-and-docking.md)：结构与图标契约、原生树、折叠及四向停靠恢复。
- [02/03 共同交付记录](verification/xml-language-tools/02-03-delivery.md)：一次共同检查、SVG/XML 短组合、双轴审查及准确议题交付状态。
- [大纲点击崩溃修复](verification/xml-language-tools/04-outline-click-crash.md)：2026-10-08 用户窗口回归、标题栏实体借用修复及真实点击验证。

- [插件 UI 解耦与文件显示布局总方案](specs/plugin-ui-decoupling.md)、[五个实施工单（#62–#66）](tickets/plugin-ui-decoupling/README.md)。已在 `codex/plugin-ui-decoupling` 完成实现、验收及双轴审查，五张工单均已推送并核对关闭。
- [工单 01：Image 与文件显示验收](verification/plugin-ui-decoupling/01-image-file-views.md)。
- [工单 02：可组合布局与提供者选择验收](verification/plugin-ui-decoupling/02-composable-layouts.md)。
- [工单 03：两组底栏与共享偏好验收](verification/plugin-ui-decoupling/03-tools-and-preferences.md)。
- [工单 04：真实插件与历史显示偏好迁移验收](verification/plugin-ui-decoupling/04-plugin-migration.md)。
- [工单 05：最终契约与集成验收](verification/plugin-ui-decoupling/05-integration-and-contract.md)。

- [Markdown 使用说明](../../plugins/markdown/README.md)、[总方案](../../plugins/markdown/docs/spec.md)、[执行工单](../../plugins/markdown/docs/tickets/README.md)、[逐单验收](../../plugins/markdown/docs/verification/README.md)、[AI 执行入口](../../plugins/markdown/AGENTS.md)。
- [插件平台规格](../specs/plugin-api-platform.md)、[平台实施工单](../specs/plugin-api-tickets/README.md)、[运行与构建](../runtime-plugins.md)、[公开协议与 SDK](../../crates/plugin-protocol/README.md)。
- [插件管理与日志方案](specs/plugin-management-logs.md)、[对应工单](tickets/plugin-management-logs/README.md)、[管理页验收](verification/plugin-management-tabs-verification.md)。
- [运行、调试与构建父议题](https://github.com/T-miracle/Editor/issues/48)、[接手终验与工单处置](verification/run-debug-build-completion-2026-10-06.md)、[B1 UI 修复与复验](verification/run-debug-build-ui-fix-2026-10-06.md)、[历史验收](verification/run-debug-build.md)、[整批初审记录（2026-10-05）](verification/run-debug-build-review-2026-10-05.md)。当前进度以终验及复验记录为准。
- [本批调试器、受控传输与单目标握手决定](specs/run-debug-build-debugger.md)。
- [构建与运行配置 B2：IDEA 参考设计提案](specs/run-config-idea-design.md)（用户已选 A / 方案 1）；[B3：双栏简洁版](specs/run-config-simple-design.md)（已落地原生弹窗，支持插件默认目标）；[B3 实现与验收](verification/run-config-simple-2026-10-06.md)。设计图与真实验证的范围在对应记录中区分。
- [运行配置重构：插件模板、原生表单与配置树](specs/run-config-plugin-tree.md)／[规格 #67](https://github.com/T-miracle/Editor/issues/67)（2026-10-07 产品基线及测试入口已确认）。新规格替代 B3 的表单、目录组织、提交和存储约定，不将旧验收计作新功能通过。
- [运行配置重构实施工单](tickets/run-config-plugin-tree/README.md)：4 个端到端切片 #68–#71 的实现与验收均完成，普通提交及 GitHub 交付记录见各工单。30 个验收场景的实际包、Windows 与真实调试证据见 [验收记录](verification/run-config-plugin-tree.md)。2026-10-07 用户另行授权合并主分支并推送，独立候选的冲突处理和验证见 [主分支集成记录](verification/run-config-main-merge-2026-10-07.md)。

平台历史资料当前仍跟踪在 `docs/specs/`；后续重归档应同步修改本入口及消费者链接，不能依赖其他聊天尚未提交的副本。
