# 02：主代理 Windows 原生交互记录

日期：2026-10-09。状态：首轮完成，等待最终候选复核；不作为工单关闭依据。

## 首轮输入

- 宿主副本：`C:/Users/Tmiracle/.codex/worktrees/plugin-api-interaction/native-qa-02/bin/editor-app.exe`，SHA256 `0678719fe9bea353893eb89944cd003f6d8320ca3bc6bc519c5b68cd7fde1480`。
- 首轮 SDK：`bda805cbcc075d56e49e55d54c11946f3f073346fd52f465d271fcd242b1b8be`。
- `example` 0.4.0 经该宿主 `--plugin-dev` 构建和激活，候选摘要 `e4632c443eced69c720f55612cf093ec1c80c3a59da71c16f5cf6f2750ae0e65`。
- 工作区和配置均位于 `C:/Users/Tmiracle/.codex/worktrees/plugin-api-interaction/native-qa-02/`；输入夹具含 120 行中文、emoji、CRLF。
- 原生操作使用 Computer Use；仅操作本次创建的独立 Nanobug 实例。

## 已观察行为

1. 编辑区右键菜单中的 `Guided check` 启动 QuickPick。Down 选择 Full，Return 确认并进入 Input，结果按选项 ID 传递。
2. Input 中真实按键 `n`、`i` 激活 Windows 中文输入法候选窗口。组合期间 Return 提交当前组合，Input 保持打开，没有提前确认请求。
3. 新一次检查中按 `n`、`i`、Space 选中“你”，再按 Return，Confirm 显示 `你 (brief)`，证明实际 IME 提交值沿正式插件任务到达后续阶段。
4. 确认后出现带 `example` 来源的 50% 非模态进度；取消后面板显示 `Cancelled`、`reason: cancelled`、`effect: not_executed`，进度控件消失。
5. Input 超过 30 秒期限后关闭并返回 `timed_out`，没有把空字符串当成成功输入；模态结束恢复原入口焦点，通知和进度没有抢走编辑器焦点。
6. 浅色和深色主题中的 Input、按钮、通知及进度均可读；Input、Confirm、Cancel 暴露可读辅助功能名称。

## 首轮发现与候选复核

- 插件面板标题命令菜单定位到了窗口左上角。已修复真实按钮布局边界采集和锚点更新，菜单模块 4 项测试通过；最终二进制仍需原生定位复核。
- QuickPick 选项的辅助功能名称为空。实施代理已补选项 label，并在补第 512 项键盘导航与滚动回归；最终候选必须复核可读名称与 IME，不能直接沿用旧 Input 输入证据。
- 宿主直接触发的 typed command 选择文件须由不可伪造的调用来源、单跳与有效权限共同限定。实际公共管理器成功/跨插件拒绝测试由工单 02 记录，不以 UI 演示代替授权验证。
- 原生文件/目录/保存选择器、取消/退役清理以及最终 SDK 和三类旧消费者包，按工单记录补齐；本轮没有把尚未执行项标记通过。

旧验收实例已关闭。新的候选在独立运行目录复核，源码或 SDK 变化后重新记录二进制和包 hash。
