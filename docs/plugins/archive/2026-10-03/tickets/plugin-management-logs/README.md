# 插件管理与运行日志改造工单

状态：01 / #23 已完成；02 / #24 为下一实施前沿，03 / #25 依赖 02。日期：2026-10-03。
依据：[规格](../../specs/plugin-management-logs.md)。已发布至 T-miracle/Editor，均使用 ready-for-agent 标签。方案 [#22](https://github.com/T-miracle/Editor/issues/22)；工单 01–03 分别为 [#23](https://github.com/T-miracle/Editor/issues/23)、[#24](https://github.com/T-miracle/Editor/issues/24)、[#25](https://github.com/T-miracle/Editor/issues/25)。原生阻塞关系已读回确认，完整 ID 映射见 [publication.json](publication.json)。

| 工单 | 阻塞项 | 端到端交付 | 状态 |
| --- | --- | --- | --- |
| [01：分栏浏览插件说明与运行信息](01-management-tabs.md) | 无 | 用户可操作六个 Tab、固定顶部、阅读概览和现有运行信息，重启及启用行为保持可用 | completed |
| [02：查看完整日志并识别未读异常](02-runtime-logs.md) | 01 | 实际插件和服务消息进入有界日志，展示时间与来源，Tab 未读图标随查看消退 | ready-for-agent |
| [03：底栏持续提醒与摘要直达日志](03-status-popover.md) | 02 | 错误/警告/loading 优先级、A 方案摘要、点击定位、跨入口已读与提醒确认闭环 | ready-for-agent |

依赖为 01 → 02 → 03，仅列直接阻塞项；03 不重复列传递依赖 01。每个切片包含必要状态、接入、UI 和行为验证，完成后可单独演示。
没有需另设工单的广域预重构；必要局部抽取先保持行为并验证，再完成所在切片。不自动启动并行代理。

已确认测试边界：实际插件包经公开管理器、后台发布进入原生界面；复用现有启动、恢复与组合 UI 夹具，不新增插件专属测试 API。
本批独立于已完成平台工单 01–20。发布前已核对重复议题，发布后已核对标签与原生阻塞关系；未关闭或改写父议题。01 的实现和验收见[分栏验证](../../verification/plugin-management-tabs-verification.md)，后续两张工单保留未完成状态。执行交付遵循已确认的提交、推送与关闭授权。
