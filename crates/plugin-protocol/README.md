# 插件 SDK

本 crate 定义版本化插件接口（WIT、Rust 类型与消息契约）。面向读者的契约文档逐步迁移至文档站点：

- 站点英文权威源：`website/src/content/docs/en/sdk/`；中文译文位于对应 `zh-cn/sdk/`。
- 编辑器通过 `sdk_export.rs` 将契约文档编入可执行文件，供 `--export-plugin-sdk` 与 `--plugin-cargo` 使用；移动文档时必须同步内嵌路径。
- 本次新增专题 `TOOLS.md`、`VIEWPORT.md`、`CODE_HIGHLIGHTING.md`、`NAVIGATION.md` 仍由此目录导出；文件布局、图片、工具组和已退役接口的概述同步维护于站点原生 UI 页。
- 站点约定见 `website/AGENTS.md`，内部规格与验收见 `docs/plugins/` 和 `docs/website/`。

接口文件仍在本目录：

- `wit/plugin.wit`：WebAssembly Component Model 导入与导出。
- `src/`：跨接口传递的 JSON 消息、文档与权限名称。

修改站点协议文档时，英文与中文必须在同一次提交中同步。SDK 内容摘要随契约和文档变化更新。
