# 终端 WebAssembly 插件

0.5.1 使用清单 protocol 4：侧边 Tab 栏、菜单、重命名、滚动和拖动行为由主程序原生模块提供，菜单样式与资源管理器右键菜单共用。插件通过 `src/controls.rs` 提交条目并处理类型化事件；Alacritty 核心、屏幕网格、滚动历史、会话状态和 Shell 配置仍由插件持有。需要使用支持 protocol 4 的新版编辑器。架构与包格式见 [运行时插件说明](../../docs/runtime-plugins.md)。

## 安装与更新

本目录同时包含插件源码与打包清单：`src/` 为终端功能实现，`Cargo.toml` 为 WASM crate，`manifest.json` 为安装声明，终端核心使用目录内的 `vendor/alacritty_terminal`（上游 0.26.0 的 WASM 适配版），Shell 目录元数据使用 `vte`，许可文本随插件包分发。编译接口由编辑器自动缓存和注入，不需要同级 `sdk/` 或主程序源码。独立构建使用 `editor-app.exe --plugin-cargo terminal/Cargo.toml build --target wasm32-wasip2 --release`。Cargo 包名仍为 `terminal-guest`。

执行 `./scripts/build-plugins.ps1` 生成标准 ZIP 包 `dist/plugins/terminal.zip`。点击设置左侧的插件图标，在“插件管理”弹窗中选择该包，确认来源与权限。也可使用“查看随附插件”。安装后立即显示底部终端，无需重启编辑器。

同 ID 包执行更新。更新前保存会话并关闭旧 Shell 及其子进程；恢复 Tab、配置和旧输出后启动新 Shell，不重放旧命令。恢复保留原有网格尺寸、文字样式、软换行、光标位置与历史滚动位置，不插入提示文字或新增命令行；旧格式中自动生成的恢复分隔行会在迁移时清除。验证失败保留旧版；切换后失败会重新启用旧版。正常关闭编辑器后再次打开也会恢复这些数据。

## 界面与操作

- 新安装终端后面板默认隐藏，可从底部工具栏的终端按钮打开；按钮在面板显示时使用选中样式。终端面板位于底部，使用编辑器共享的 Dock，可拖动调整尺寸。隐藏不停止程序。
- 右侧竖向 Tab：普通新建统一使用 Shell 工具名，例如多个 PowerShell 都叫 `powershell`，不再追加数字；宿主传入名称时使用该名称。已有会话的原名称不变。Tab 通过独立 ID 区分，允许名称重复；支持切换、关闭、拖动排序、双击改名。改名时自动聚焦、全选且不显示输入框边框，Enter 或失焦保存；过长名称省略显示，不遮挡关闭按钮。拖动终端内容与 Tab 栏之间的分隔线可调整 Tab 栏宽度，该宽度随会话恢复。Tab 过多时可在列表上滚动。
- 右侧 Tab 栏的左边缘内侧可拖动调整宽度，悬浮时显示横向调整指针。选中 Tab 与命令区同色相连，其左侧不绘制贯穿边线；其余列表和空白区域保留边框。命令区有内边距，滚动条贴近右侧边界，使用与资源管理器相同的控件；有历史时滚动会显示滚动条，停止滚动 1 秒后隐藏，没有有效历史时不显示。
- 标题栏 `+` 新建，`⌄` 展开 Shell 与终端操作，`≡` 显示插件声明的命令，`—` 隐藏面板。较矮面板中的菜单可滚动。
- “运行项目”先请求编辑器保存当前文件，再新建会话执行 `run_command`。未配置时检测 Rust 的 `cargo run`，或 Node 的 `npm run dev` / `npm start`。
- “在当前文件目录打开”使用当前文件父目录；“发送选中内容”把编辑器选区作为粘贴发送，不额外追加回车。
- 支持中文输入、复制粘贴、选择、滚动历史、ANSI/真彩色、备用屏幕、应用光标模式及基础鼠标协议。TUI 占用鼠标时可按 Shift 使用本地选择与滚动。
- 默认光标为竖线；Shell 或 TUI 明确设置的光标形状仍会生效。空白行、文字末尾的空白网格和面板内边距不能发起本地选区，文字之间的空格仍可选择。
- 命令行区域右键菜单提供“复制”“粘贴”“清空缓冲区”。没有选中的非空文字时“复制”禁用；清空会移除当前会话的所有可见输出和滚动历史，保留正在运行的 Shell。TUI 启用鼠标协议时，按 Shift+右键打开本地菜单。
- 默认 PowerShell 会保留原提示符，并通过不可见元数据追踪当前目录。其他未发送目录元数据的 Shell 保存启动目录。

| 快捷键 | 操作 |
| --- | --- |
| Ctrl+Shift+T / W | 新建 / 关闭会话 |
| Ctrl+PageUp / PageDown | 上一个 / 下一个会话 |
| Ctrl+C / V | 复制选中文字 / 粘贴 |
| Ctrl+Shift+C / V | 复制 / 粘贴的兼容快捷键 |
| Ctrl+Alt+Enter | 在当前文件目录新建会话 |
| Ctrl+Shift+Enter | 发送编辑器选区 |
| Ctrl+` | 打开插件管理弹窗 |

上述复制和粘贴快捷键在命令行区域获得焦点时生效。Ctrl+C 不再发送中断信号；需要停止正在运行的命令时，打开标题栏 `⌄` 菜单并选择“中断”。Shell 的 Tab 补全、上下方向键历史和其他输入快捷键仍按所用 Shell 的规则工作。

## 宿主主动调用

编辑器功能可以调用通用的 `EditorApp::invoke_plugin_command`，无需依赖终端源码，也无需先打开终端面板。成功排队后宿主显示插件的首个面板并聚焦；后台执行失败通过现有插件状态报告。插件必须已经安装、启用并完成加载；调用不能绕过授权或自动启用插件。非 GUI 宿主可用 `plugin_runtime::Manager::invoke_command`。

```rust
// 在编辑器功能中创建一个指定名称和工作目录的终端。
self.invoke_plugin_command(
    "me.terminal",
    "terminal.new",
    serde_json::json!({ "name": "构建输出", "cwd": project_directory }),
    window,
    cx,
)?;

// 后续运行项目功能可复用此入口：先保存当前文件，再新建具名终端并运行命令。
self.invoke_plugin_command(
    "me.terminal",
    "terminal.run",
    serde_json::json!({ "name": "运行 Editor", "cwd": project_directory, "command": "cargo run" }),
    window,
    cx,
)?;
```

| 参数 | 含义 |
| --- | --- |
| `name` | 可选 Tab 名称；去除控制字符及首尾空白，最多 80 个字符；空名称使用工具名 |
| `cwd` | 可选启动目录；缺省使用工作区目录 |
| `profile` | 可选 Shell 配置索引，从 0 开始；缺省使用 `default_profile` |
| `command` | `terminal.run` 的可选运行命令；缺省使用 `run_command` 或检测当前工作区的项目类型；`terminal.new` 不执行命令 |

这些参数由终端插件的 `src/commands.rs` 解释。宿主只发送 `Event::Command.arguments`，检查声明命令与 64 KiB 参数上限，不解释终端字段。没有参数的旧宿主调用保持兼容；新的宿主调用 API 需要使用更新后的编辑器程序。参数类型错误不会创建会话；到达会话上限或禁止新建时，不会把命令误发到现有 Tab。等待保存结果的运行请求不写入快照，更新后不会自动执行。

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

JSON 不支持注释。`enabled` 控制是否允许新建会话，正式停用请使用插件管理；`default_profile` 从 0 开始；字号为 8–32，历史最多 100000 行。颜色使用 `#RRGGBB`，`ansi` 可填 16 个颜色。`run_command` 仅在用户执行“运行项目”时运行。字体和颜色设置作为插件备用值；当前编辑器主题提供同名属性时，主题优先。

终端插件内置与 Editor 浅色、深色代码区域相配的两套配色，随编辑器主题模式切换。编辑器内置的浅色、深色主题也各自声明完整的 16 色 ANSI 覆盖。当前主题的 `plugins["me.terminal"]` 颜色优先；未声明的 ANSI 色依次取插件私有配置和插件内置配色。默认文字、背景、光标与选区优先取当前主题的专属颜色或通用颜色。当前主题的字体角色优先于插件私有字体设置。

**外部主题对接接口：**安装并启用的编辑器主题包与内置主题使用同一套 `themes[].plugins` 契约。宿主自动把当前主题的通用颜色、字体及插件专属样式放入 `Environment`，切换主题或更新主题包时发送 `Event::Theme`；终端收到后立即重算样式，无需主题包直接调用终端插件。主题包可按浅色和深色分别覆盖终端颜色与字体，例如：

```json
"plugins": {
  "me.terminal": {
    "typography": {
      "content": { "family": "Cascadia Mono", "size_px": 15 },
      "tab": { "family": "Segoe UI", "size_px": 14, "bold": true },
      "menu": { "family": "Segoe UI", "size_px": 14 },
      "error": { "family": "Segoe UI", "size_px": 13 }
    },
    "ansi": {
      "yellow": "#795100",
      "dim_red": "#9D4A43"
    },
    "selection": "#D4E2FF",
    "ui": {
      "tab_bar": { "background": "#F7F8FA" },
      "menu": { "background": "#FFFFFF" }
    }
  }
}
```

终端只读取 `plugins["me.terminal"]` 下的配置。外部主题未声明的 ANSI 键继续使用插件私有配置或内置配色，不继承内置编辑器主题的 ANSI 覆盖；通用背景、文字等颜色仍取当前外部主题。`typography` 的 `content`、`tab`、`menu`、`error` 分别控制终端内容、标签、菜单和错误文字；各角色未声明的字体属性继承主题的 `typography.mono` 或 `typography.ui`。可覆盖的终端文字类型与主题键如下；每个键都是独立的 `#RRGGBB` 值，省略时继承插件或编辑器默认值。

| 终端内容 | `plugins["me.terminal"]` 下的主题键 |
| --- | --- |
| 默认文字、背景、光标、光标内文字、选区背景 | `foreground`、`background`、`cursor`、`cursor_text`、`selection` |
| ANSI 标准文字 0–7 | `ansi.black`、`ansi.red`、`ansi.green`、`ansi.yellow`、`ansi.blue`、`ansi.magenta`、`ansi.cyan`、`ansi.white` |
| ANSI 高亮文字 8–15 | `ansi.bright_black`、`ansi.bright_red`、`ansi.bright_green`、`ansi.bright_yellow`、`ansi.bright_blue`、`ansi.bright_magenta`、`ansi.bright_cyan`、`ansi.bright_white` |
| ANSI 弱化文字 | `ansi.dim_black`、`ansi.dim_red`、`ansi.dim_green`、`ansi.dim_yellow`、`ansi.dim_blue`、`ansi.dim_magenta`、`ansi.dim_cyan`、`ansi.dim_white` |
| 加粗、弱化的默认文字 | `bright_foreground`、`dim_foreground` |
| 256 色索引 16–255 | `indexed.16` 至 `indexed.255`，可只覆盖需要调整的索引 |

终端自行绘制的界面也可在 `plugins["me.terminal"].ui` 下逐项覆盖，省略时沿用当前编辑器通用颜色或终端默认色：

| 终端界面 | `ui` 下的主题键 |
| --- | --- |
| 标签栏底色、外侧分隔线 | `tab_bar.background`、`tab_bar.border` |
| 标签行分隔线 | `tab.border` |
| 选中标签底色、文字 | `tab.active.background`、`tab.active.foreground` |
| 未选中标签底色、文字 | `tab.inactive.background`、`tab.inactive.foreground` |
| 关闭按钮底色、文字 | `tab.close.background`、`tab.close.foreground` |
| 标签重命名输入框底色、文字 | `tab.rename.background`、`tab.rename.foreground` |
| 终端菜单底色、文字 | `menu.background`、`menu.foreground` |
| 错误提示文字 | `error.foreground` |

终端所在 Dock 的标题栏、切换按钮和滚动条由编辑器绘制，外部主题通过通用 `colors` 与 `components` 中的 `dock-title-bar`、`dock-tab`、`panel-toggle`、`scrollbar` 等项控制。标签重命名输入框由宿主提供原生编辑能力，终端将 `tab` 字体及 `tab.rename.*` 颜色交给宿主绘制。

ANSI 颜色 0–15 同时可用于前景或背景；加粗、弱化属性将标准前景文字映射到对应的高亮色或弱化色。未覆盖的 256 色索引仍按标准色盘计算。应用自行指定的 RGB 真彩色保持原值，Shell 通过 OSC 动态设定的颜色优先于主题。其他界面文字和边框继续使用编辑器提供的通用主题颜色。

历史还受快照总容量限制，超额时舍弃最旧输出。旧输出以带颜色的显示内容保存，不包含可自动执行的 Shell 命令。卸载时可选择保留或删除会话和配置。

## 验证范围

`./target/release/editor-app.exe --plugin-cargo plugins/terminal/Cargo.toml test --lib` 验证输入编码、Tab 与快照、目录追踪、边框和滚动。`cargo test -p editor-app extensions::tests` 验证动态停靠、中文输入与原生编辑提交。实际 ZIP 包 + ConPTY 测试为 `cargo run -p plugin-runtime --example smoke -- dist/plugins/terminal.zip`。

Windows 已进行实际运行验证。图片协议、kitty 扩展键盘协议和 Shell 命令检测不在此版本内；没有逐一验证所有第三方 TUI。OSC 52 剪贴板访问不启用，复制粘贴由用户操作触发。

0.4.0 恢复 Alacritty 终端核心，保持 WASM 架构及 schema 1 快照兼容。调整历史容量后，当前主屏历史立即受限。


0.4.1 修复较长滚动历史在定期保存或卸载时耗尽 WASM 执行预算的问题。可在构建插件包后执行 `cargo run -p plugin-runtime --example diagnose_snapshot`；设置 `DIAG_WIDE=1` 可验证长行历史。

0.4.2 将用户提供的终端 SVG 用于面板标题和底部切换按钮；浅色主题显示黑色图形，深色主题显示白色图形。更新已安装的插件时，选择新生成的 `terminal.zip`。

0.4.9 为终端加入匹配 Editor 的浅色、深色命名色与弱化色，并支持编辑器主题逐项覆盖。

0.4.10 默认使用竖线光标，并限制本地选区只从有文字的行内开始，不再高亮空白网格。

0.5.2 恢复输出时保留画面、光标与滚动位置，不再加入恢复提示或额外换行；Ctrl+C/V 直接复制、粘贴，命令区右键菜单提供复制、粘贴和清空所有缓冲区内容。

0.5.3 新终端使用工具名或宿主指定名称，不再递增命名。宿主可通过通用命令 API 主动创建终端或传入运行任务，名称、目录及 Shell 选择由终端插件处理。
