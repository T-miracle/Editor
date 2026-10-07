# 原生 UI 文档

本目录供 AI 与维护者保存原生 UI 规格、工单与行为验收证据。设计确认或工单发布不代表功能已通过验收。

- [资源管理器文件转移规格](specs/explorer-file-transfer.md)
- [资源管理器文件转移工单](tickets/explorer-file-transfer/README.md)
- [资源管理器文件转移验收](verification/explorer-file-transfer.md)：代码和自动化验证已完成，原生拖放等验收仍待补充。
- [快捷键面板与用户绑定规格](specs/keyboard-shortcuts.md)：交互、C 视觉方案及应用级测试接缝已确认。
- [三张实施工单 #81–#83](tickets/keyboard-shortcuts/README.md)：按 #81 → #82 → #83 完成，已推送主分支并核对关闭。
- [快捷键验收记录](verification/keyboard-shortcuts.md)：记录 T01–T22 实际证据、失败修复与验证限制；最终应用测试和必要原生组合已通过，父规格 #77 保持打开。

方案放入 `specs/`，工单放入 `tickets/`，验收记录放入 `verification/`。不得用生成式原型图替代真实按键、点击、配置重载与插件生命周期验收。

返回[项目文档入口](../README.md)。
