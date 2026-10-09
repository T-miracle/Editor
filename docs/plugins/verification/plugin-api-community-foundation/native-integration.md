# 01＋02：主代理 Windows 原生整合验收

日期：2026-10-09。生产代码候选 `325c9d701ac46f2755349ab8f22cdce294efd8ef`；新增只读左栏菜单与菜单焦点退役连接通过。两单原有完整矩阵引用逐单记录，本文件不替代本地左栏版本冲突的 actual SDK / GPUI 回归。

## 输入与隔离

- 自有宿主副本 `C:/Users/Tmiracle/.codex/worktrees/plugin-api-community/native-qa-integration/bin/editor-app-doc-menu-final.exe`，SHA256 `ee95fa6057a1c50f29dfdcaebe92e362ddd645ddfee8224568984390ee971e4e`；SDK key `34b1ef263fbab5727e7d145de7b16d710a2fd5d9b093750b36a5e6355b4a0ead`。
- 旁置只读菜单验收包 `document-menu-reader-0.18.0.zip` 经公开 shipped_roots、普通包验证和 Manager 启动；ZIP SHA256 `4f04224a5eeabb6c4092a18bf7bf7de44fd5eef788cc3083675f691f005dd148`，实际安装 WASM SHA256 `66c6492f4b860b4b89e0e439b9ef4c8c42e214af64dab52e22b10f42b2f1f557`。它使用 capability-example 原字节，仅清单声明独立 ID 和只读条件菜单，没有访客或宿主专用分支。
- history-preview 0.1.2 从正式包准备公开静态插件项目，通过 `--plugin-dev` 激活；实际开发候选 `b7e5a30660167b0ddee4a628c84cc9007fa5fcb7292eed08b166a0698cfe6bd3`，WASM SHA256 `6e75dd59be5d0e2dfad3723ecce4ae4b211fd1c8e3fee32d99f495252aa2d8cf`，与新正式 history ZIP 内字节一致。只授予 `editor.read`、`ui.panels`，reload 无须重复编译预编译访客。
- 独立 `native-qa-integration/profile-final`、已知 QA 工作区 `native-qa-integration/workspace`；日志 `native-qa-integration/controller-final.log`。磁盘 `current.txt` 初始 SHA256 `9819d5666deeeacd6dc57eb35b0ff3ae6a6a2c4c89af52787a41eca218a3f72f`。

## 实际观察

1. 原生编辑区实际点击、Ctrl-A、输入 `当前未保存内容：中文😀菜单整合`；标签显示 dirty。历史插件菜单实际“Compare historical content”打开比较：左侧只读历史含中文与非 BMP，右侧为当前未保存内容。Windows UIA 左侧为只读 Document，ID `document-diff-left-input`；右侧为可编辑 Edit。
2. 实际点击左侧、Ctrl-A、右键，并点击 Base 菜单的第一条“Read document context”。能力访客在面板绘制 JSON 回执：`has_selection=true`、`writable=false`、`directory=false`、`language="text"`、`extension=null`、`path=null`。当前右侧磁盘路径没有串入虚拟左目标。
3. 再次实际右键左栏，Down 将高亮移到第二个菜单行，证明菜单接收键盘导航。保持此菜单焦点，TTY 控制器发送 `reload`；代次 1 激活后，比较视图、只读虚拟标签和原菜单撤下，只剩当前本地文档。
4. 退役后没有再次点击编辑区，直接 Ctrl-A 选中右侧文档，再输入 `退役后无需点击：右侧中文😀输入成功`。实际截图与 Windows UIA Edit Value 均一致；访客随后收到当前本地文档的选择事件，失效左栏没有继续接收输入。
5. 控制器正常 `stop`、退出 0，精确自有 EXE 进程数为 0。磁盘 `current.txt` 的 SHA256 与初始相同，未保存输入没有隐式写盘。

## 范围与工具状态

只读文档角色、键盘选择、实际菜单行、虚拟上下文、拥有焦点的菜单退役及无 reclick 输入由本次物理连接覆盖。本地左栏的 cwd 同名路径和旧 revision 拒绝由同候选的三项 actual SDK / GPUI 菜单测试覆盖；不为原生验收新增宿主测试接口。深浅主题、其他控件、完整 IME/缩放及选择权限矩阵引用 01、02 的相同接缝记录。

共享桌面另一独立 QA 曾获得焦点；Computer Use 也返回过期元素/截图或 active-request。主代理丢弃旧输入目标，重新选择精确自有 EXE，并只在当前自有截图上行动；没有操作其他 QA 窗口。窗口激活不重新点击编辑区，故不替代第 4 步的焦点恢复。异步 UIA 的首次树有前帧信息；上述文档 Value 来自实际稳定观察，不宣称每次事件都即时播报。
