# 终端 WebAssembly 插件

终端的Alacritty 终端核心、屏幕网格、滚动历史、Tab、主题、Shell 配置和交互全部运行在 `plugins/terminal`。主程序只提供受控 PTY、编辑器操作和通用 GPUI 绘制接口。架构与包格式见 [运行时插件说明](../../docs/runtime-plugins.md)。

## 安装与更新

本目录同时包含插件源码与打包清单：`src/` 为终端功能实现，`Cargo.toml` 为 WASM crate，`manifest.json` 为安装声明，终端核心使用目录内的 `vendor/alacritty_terminal`（上游 0.26.0 的 WASM 适配版），Shell 目录元数据使用 `vte`，许可文本随插件包分发。仅共享宿主协议引用仓库的 `crates/plugin-protocol`。Cargo 包名仍为 `terminal-guest`，已有构建命令不变。

执行 `./scripts/build-plugins.ps1` 生成标准 ZIP 包 `dist/plugins/terminal.zip`。点击设置左侧的插件图标，在“插件管理”弹窗中选择该包，确认来源与权限。也可使用“查看随附插件”。安装后立即显示底部终端，无需重启编辑器。

同 ID 包执行更新。更新前保存会话并关闭旧 Shell 及其子进程；恢复 Tab、配置和旧输出后启动新 Shell，不重放旧命令。验证失败保留旧版；切换后失败会重新启用旧版。正常关闭编辑器后再次打开也会恢复这些数据。

## 界面与操作

- 终端面板默认在底部，使用编辑器共享的 Dock，可拖动调整尺寸。状态栏面板按钮控制显隐，隐藏不停止程序。
- 右侧竖向 Tab：默认 `powershell`、`powershell2`、`powershell3`；支持切换、关闭、拖动排序、双击改名，Enter 或失焦保存。Tab 过多时可在列表上滚动。
- 选中 Tab 与命令区同色相连，其左边没有分隔线；其余列表和空白区域保留边框。命令区有内边距，滚动条贴近右侧边界，使用与资源管理器相同的控件；有历史时滚动会显示滚动条，停止滚动 1 秒后隐藏，没有有效历史时不显示。
- 标题栏 `+` 新建，`⌄` 展开 Shell 与终端操作，`≡` 显示插件声明的命令，`—` 隐藏面板。较矮面板中的菜单可滚动。
- “运行项目”先请求编辑器保存当前文件，再新建会话执行 `run_command`。未配置时检测 Rust 的 `cargo run`，或 Node 的 `npm run dev` / `npm start`。
- “在当前文件目录打开”使用当前文件父目录；“发送选中内容”把编辑器选区作为粘贴发送，不额外追加回车。
- 支持中文输入、复制粘贴、选择、滚动历史、ANSI/真彩色、备用屏幕、应用光标模式及基础鼠标协议。TUI 占用鼠标时可按 Shift 使用本地选择与滚动。
- 默认 PowerShell 会保留原提示符，并通过不可见元数据追踪当前目录。其他未发送目录元数据的 Shell 保存启动目录。

| 快捷键 | 操作 |
| --- | --- |
| Ctrl+Shift+T / W | 新建 / 关闭会话 |
| Ctrl+PageUp / PageDown | 上一个 / 下一个会话 |
| Ctrl+Shift+C / V | 复制 / 粘贴 |
| Ctrl+Alt+Enter | 在当前文件目录新建会话 |
| Ctrl+Shift+Enter | 发送编辑器选区 |
| Ctrl+` | 打开插件管理弹窗 |

## 自定义配置

通过“主题 / 配置”打开插件私有 `settings.json`，保存后执行“重新加载终端配置”。默认路径：`%APPDATA%/MeEditor/runtime-plugins/data/me.terminal/settings.json`。配置放在用户目录，不从打开的项目自动读取。

```json
{
  "enabled": true,
  "font_family": "Cascadia Mono",
  "font_size": 14,
  "history": 10000,
  "default_profile": 0,
  "profiles": [
    { "name": "PowerShell", "program": "powershell.exe", "args": ["-NoLogo"] },
    { "name": "CMD", "program": "cmd.exe", "args": ["/d"] }
  ],
  "run_command": null,
  "theme": {
    "background": "#181B22",
    "foreground": "#D8DEE9",
    "cursor": "#88C0D0",
    "selection": "#3B4252",
    "ansi": null
  }
}
```

JSON 不支持注释。`enabled` 控制是否允许新建会话，正式停用请使用插件管理；`default_profile` 从 0 开始；字号为 8–32，历史最多 100000 行。颜色使用 `#RRGGBB`，空值表示继承编辑器主题；`ansi` 可填 16 个颜色。`run_command` 仅在用户执行“运行项目”时运行。

历史还受快照总容量限制，超额时舍弃最旧输出。旧输出以带颜色的显示内容保存，不包含可自动执行的 Shell 命令。卸载时可选择保留或删除会话和配置。

## 验证范围

`cargo test -p terminal-guest --lib` 验证输入编码、Tab 与快照、目录追踪、边框和滚动。`cargo test -p editor-app extensions::tests` 验证动态停靠、中文输入与原生编辑提交。实际 ZIP 包 + ConPTY 测试为 `cargo run -p plugin-runtime --example smoke -- dist/plugins/terminal.zip`。

Windows 已进行实际运行验证。图片协议、kitty 扩展键盘协议和 Shell 命令检测不在此版本内；没有逐一验证所有第三方 TUI。OSC 52 剪贴板访问不启用，复制粘贴由用户操作触发。

0.4.0 恢复 Alacritty 终端核心，保持 WASM 架构及 schema 1 快照兼容。调整历史容量后，当前主屏历史立即受限。


0.4.1 修复较长滚动历史在定期保存或卸载时耗尽 WASM 执行预算的问题。可在构建插件包后执行 `cargo run -p plugin-runtime --example diagnose_snapshot`；设置 `DIAG_WIDE=1` 可验证长行历史。

0.4.2 将用户提供的终端 SVG 用于面板标题和底部切换按钮；浅色主题显示黑色图形，深色主题显示白色图形。更新已安装的插件时，选择新生成的 `terminal.zip`。
