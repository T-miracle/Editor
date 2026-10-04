# 插件 SDK

本 crate 定义的版本化插件接口（WIT、Rust 类型与消息契约）保留在此目录；**面向读者的
契约文档已移到文档站点**，源码旁不再保留副本：

- 站点（唯一手写源，英文权威版）：`website/src/content/docs/en/sdk/`
- 编辑器把这份文档编进可执行文件，并在 `--export-plugin-sdk` / `--plugin-cargo` 时交给
  插件项目；因此站点目录被移动或改名会直接导致宿主编译失败，这是有意建立的耦合。
- 站点约定见 `website/AGENTS.md`；方案、规格与工单见 `docs/website/`。

接口文件保持在本目录：

- `wit/plugin.wit`：WebAssembly Component Model 的导入与导出。
- `src/`：跨接口传递的 JSON 消息、文档与权限名称。

修改协议时先改站点英文页，再同步中文译文，两边必须在同一次提交内完成。
