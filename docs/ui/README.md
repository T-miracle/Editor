# 原生 UI 文档目录

本目录保存原生 UI 方案，以及 GPUI 控件、输入与布局相关调研。方案和调研按各自状态、日期与依赖版本理解，不代表相关功能已实现或验收。

- [宿主消息窗口规格](specs/host-messages.md)：[两张实施工单](tickets/host-messages/README.md)已实现、验收、审查并推送，GitHub #85、#86 已核对关闭；证据见 [实施验收](verification/host-messages.md)。
- [宿主消息来源盘点](verification/host-messages-sources.md)：已接入来源、插件归属及排除依据。
- [运行截图反馈修正](verification/host-messages-feedback.md)：日常消息范围、扫帚按钮、窗口工具组与底部文字移除的后续验收，不改写原始工单证据。

- [快捷键面板与用户绑定规格](specs/keyboard-shortcuts.md)：[三张实施工单 #81–#83](tickets/keyboard-shortcuts/README.md)已交付并核对关闭；实际证据与限制见[验收记录](verification/keyboard-shortcuts.md)。
- [资源管理器外部粘贴、拖放与文件操作撤销规格](specs/explorer-file-transfer.md)：[3 张工单 #78–#80](tickets/explorer-file-transfer/README.md)代码已合并，自动化验证与审查已完成，原生验收待补充；工单保持打开，见[验收记录](verification/explorer-file-transfer.md)。

- [GPUI 鼠标滚轮惯性与顺滑滚动调研](research/GPUI滚轮惯性调研.md)：2026-09-23 的研究记录。

后续 UI 方案、工单与验收按需放入 `specs/`、`tickets/`、`verification/`；研究资料放入 `research/`，并更新本索引。

返回[文档总目录](../README.md)。
