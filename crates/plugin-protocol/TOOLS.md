# 插件工具与共享偏好

`ui.tools ^1` 为 `Document.tools` 提供至多 32 个原生底栏按钮。
每项包含稳定 ID、`LocalizedText` 双语名称与提示、`ToolIcon` 深浅主题包内 SVG、
`visible/selected/disabled/order` 及明确的 `ToolTarget`。
File 目标必须等于当前 `Document.file`；Window 目标只能是本贡献所属独立窗口。
窗口显隐入口来自清单中的独立面板，中心布局和 auxiliary 贡献不自动产生窗口入口。

宿主在短竖线左右分别绘制窗口组和工具组，多个适用文件贡献可以同时提供工具。
插件接收 `Notification::Tool(ToolEvent)` 并实现功能；底栏与各自溢出菜单使用同一状态和事件。
点击底栏保留此前文件或活动窗口目标，文件切换、重开、实例替换、模态、隐藏、禁用、
旧 UI revision 和旧权限均不能通过校验。实例退出撤销全部贡献。

工具图标只能引用当前包内相对路径，运行时校验规范化边界及至多 64 KiB 的安全几何 SVG。
缺失或非法资源拒绝整个发布，不使用宿主专属图标兜底；`currentColor` 接收通用原生主题颜色。
本能力不授予文件读取或进程权限。File 工具需工作区实例和 `editor.read`。

`storage.private >=1.1, <2` 增加 `ReadPreference/WritePreference`，需要 `storage` 权限。
SDK 提供 `read_preference(key, watch)` 与 `write_preference(key, expected_revision, data)`。
宿主给定插件与工作区命名空间，`PreferenceKey` 只包含规范化文件类型和插件本地名称。
名称不能指定路径或其他所有者；空文件类型可以保存工作区窗口意图。

缺失值为 revision 0/data None，写入是 compare-and-set，过期版本返回 `Conflict` 并保留新值。
相同值不推进 revision；JSON 上限 64 KiB，仍计入插件私有目录整体配额及迁移事务。
损坏记录明确报错并保留，不能静默重置。实例最多 32 个 watch，返回普通可撤销资源句柄；
关闭资源、撤销实例均清理订阅，后续变更通过 `PreferenceChanged` 发送。
损坏或不可读记录终止对应 watch 并发送 `SubscriptionFailed`。

数据是不透明显示意图，不授予实际窗口、布局或文本权力。各实例须校验订阅与 revision，
冲突后重新读取并应用当前意图，不盲目重试覆盖；原生编辑会话、选择和 Undo 保持各文档独立。
独立示例：`layout-example` 使用工具及按类型偏好；`tools-example` 同时贡献辅助工具和独立窗口。
