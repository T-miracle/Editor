# 01：主代理 Windows 原生验收记录

日期：2026-10-09。状态：首轮完成，发现项修复及候选版本复核中；本记录不代表工单已完成。

## 首轮输入

- 宿主副本：`C:/Users/Tmiracle/.codex/worktrees/plugin-api-community/native-qa-01/bin/editor-app.exe`。
- 宿主 SHA256：`2b4ae1837f39fb17cc320aae608535f7494f46c6435fe6c3b91854b615eb61ae`。
- 内嵌 SDK：`98b5137b113d5890ad0e63f26caa3d24587761ad1ce5bef3580fd4f2f2d16e4c`。
- 插件：`history-preview` 0.1.0，经该宿主 `--plugin-dev` 构建和实际激活；候选摘要 `45ddd8eb3bd01dcb61f09552ddb77dde9e880ec61729c61dbcef8c6a5144d07a`。
- 隔离目录：工作树内 `target/native01-profile-root` 与 `target/native01-workspace`，不使用日常配置或用户文档。
- 授权参数：`--grant editor.read --grant ui.panels`。
- 原生操作使用 Computer Use 的 Windows 输入、截图及 UI Automation；没有通过测试专用宿主接口调用比较。

## 已观察行为

1. 从插件面板菜单执行 `Compare historical content`，打开带“只读”标记的历史快照 Tab，呈现历史内容与 `current.txt` 的原生双栏及差异标记。中文、emoji 与 CRLF 文本可见。
2. 在右栏选择全部并输入 `未保存的右侧文本😀`、`second changed line`，本地文档标记 dirty。原比较因版本变化关闭，再次执行比较后右栏呈现最新未保存内容。
3. 点击左栏后按 Ctrl+S，消息窗口显示“此文档为只读内容，无法保存”。右栏仍为 dirty。磁盘文件操作前后 SHA256 均为 `6b1ecbe68f1053447688cb8da0426c49fbed2d8ad0571870a81c9188cf793749`，没有保存右栏或创建虚拟文件。
4. 左栏 Ctrl+A 可选择文字，输入中文及 emoji 不改变历史内容；选择和编辑权限分别成立。
5. 设置窗口切换深色主题后，主窗口、右栏与插件面板更新；左栏背景更新，但行号背景和当前行仍使用浅色样式，列为待修复。

## 首轮发现与复核要求

- 打开虚拟 Tab 后，插件运行状态出现管理器 `os error 123`。实施代理已定位到 bundled first-use 发现把虚拟 URI 作为本地路径送入文件系统。必须补真实 worker 回归，并在新候选原生窗口中确认不再出现此错误。
- 比较左栏没有同步主题设置。必须复用现有编辑器样式入口，同步两侧行号、选区、当前行、字体与差异颜色，再复核深浅主题。
- 比较绑定精确版本，源版本变化关闭，需要再次比较；该规则须在 SDK 中英文站点说明中准确表达。
- 后续候选源码、SDK 或包变化后记录新 hash，不能把本轮存在发现项的二进制当作最终通过。

控制器已通过 `stop` 正常关闭其自有实例。后续原生输入使用新的隔离运行数据。
