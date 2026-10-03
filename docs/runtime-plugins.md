# 运行时插件平台

## 原生界面协议

SVG 预览插件使用 `protocol = 6`：清单面板声明 `position = "editor"` 与 `file_extensions = ["svg"]`，宿主在匹配文件的编辑区内创建可拖动的左右分栏。获得 `editor.commands` 授权后，插件接收面板范围内的 `Event::Document { path, text }`，内容来自当前内存文档，包括未保存修改；切换到不匹配文件或隐藏预览时发送空文档并收起分栏。编辑器保留原生输入、标签、补全和诊断行为。停用、卸载和热更新沿用已有生命周期。

通用 `Paint::Svg { rect, clip, source }` 保留原有颜色与透明度；前面的绘图构成底层，SVG 可叠在棋盘上。主程序后台解析、按可见区域裁剪并缓存矢量图，UI 线程只提交图像；普通停靠插件不使用此接口。协议 1–5 继续兼容。

终端插件 0.6.0 使用 `protocol = 7` 和公开能力协议：普通 Row 组合 SideTabs 与 Canvas，画布负责字符网格，原生侧栏支持左右布局、滚动、重命名、关闭、排序和宽度调整。菜单使用共享原生控件，以稳定 ID 回传操作。主题、字体、滚动和输入均经通用 UI 契约传递。当前包安装与激活只接受协议 7；旧包保留数据并提示更新。

插件可声明 `protocol = 2`，使用 `plugin_protocol::ui::Document/Node` 返回行列布局和原生控件树，宿主通过 gpui-base 实现布局、输入、焦点、滚动和模态弹窗。协议、事件、主题角色与限制见 [插件原生界面协议](../crates/plugin-protocol/UI.md)。该文档及 Rust 接口由主程序内嵌，并自动提供给插件构建。protocol 1 的画布终端继续兼容；示例插件 0.2.0 演示新接口。

编辑器提供通用安装接口。终端和示例插件通过 WebAssembly Component Model 执行；Rust 为声明式资源加可选 WASM 策略，TOML、HTML 和 JavaScript 是声明式资源包。亮色与深色基础主题内置于编辑器，首次启动默认使用亮色主题。终端核心直接依赖支持 WASM 的上游 `term-wm-vt100`，不在项目保留 vendor 副本。语言插件包携带 Tree-sitter WASM grammar；其他主题插件仍可携带主题和图标资源，由宿主验证并注册。

## 构建与安装

```powershell
# 只需在开发机器安装一次目标工具链。
rustup target add wasm32-wasip2
# 先构建主程序，由它管理编译接口，再构建终端、示例及语言插件。
./scripts/build-plugins.ps1
# 只构建开发版编辑器。
cargo build -p editor-app
# 生成发行目录：editor-app.exe + plugins/*.zip。
./scripts/package-editor.ps1
```

开发输出包含 `dist/plugins/terminal.zip`、`example.zip`、`svg.zip`、`rust.zip`、`toml.zip`、`html.zip` 和 `javascript.zip`。插件包是标准 ZIP，都包含 `manifest.json`；可执行插件另含组件 `.wasm`，声明式插件只包含各自资源。打包后的使用者不需要 Rust、Cargo 或源代码。

面板可在清单中声明 `icon_light` 和 `icon_dark`，分别指向包内 SVG。宿主验证并读取这两份资源，在浅色、深色主题间切换时自动更新面板标题和底部切换按钮。终端插件的图标位于 `plugins/terminal/icons/`；更新已经安装的终端插件时，选择新版 `terminal.zip` 即可替换图标。

打开编辑器标题栏的“插件管理”，选择“安装 / 更新本机插件包”，确认来源及权限后即可使用。也可选择“查看随附插件”。安装成功会注册清单中的停靠面板、标题工具按钮、命令菜单和快捷键，或加载语言、主题和图标资源。终端默认停靠底部，示例包声明右侧计数器和底部笔记两个面板。面板尺寸可以像资源管理器一样拖动调整，底部按钮可以隐藏和显示各面板。

Rust、TOML、HTML 和 JavaScript 插件安装后才提供对应文件的语法、图标及声明的语言服务配置；停用或卸载会撤销这些贡献。HTML 支持 `.html` 和 `.htm`，当前不包含 LSP 或 JavaScript/CSS 嵌入高亮，详见 [HTML 插件说明](../plugins/html/README.md)。JavaScript 支持 `.js`、`.mjs`、`.cjs` 和 `.jsx`，提供独立的语法解析与高亮，详见 [JavaScript 插件说明](../plugins/javascript/README.md)。内置主题无需安装插件，也可在设置中切换亮色和深色。这些声明式包的 `contributions` 字段指向各自包内的 `plugin.toml`，由宿主直接管理安装、启用、停用和卸载；更新这些包无需重新编译编辑器。

可用 `cargo run -p plugin-runtime --example declarative_smoke -- dist/plugins` 验证这些包的安装、启用、停用和卸载。

同 ID 插件包执行更新，版本号与协议版本分别校验。编辑器进程保持运行。停用、卸载先显示关闭程序确认；卸载可以保留或删除该插件的全部私有数据，插件代码包会移除。保留数据后重装可以恢复。

## 模块职责

| 位置 | 拥有的实现 |
| --- | --- |
| `crates/plugin-protocol` | 主程序持有的版本化 WIT 契约、权限名称、消息、通用界面描述及快照信封 |
| `crates/plugin-runtime` | Wasmtime 隔离、受控资源句柄、安装事务、快照存储和插件生命周期 |
| `crates/editor-app/src/extensions` | 通用 GPUI 绘制、原生输入法、控件与滚动条、停靠注册、权限与管理界面 |
| `plugins/terminal` | Shell 配置、项目运行、Tab 与命名、输入协议、选择、VT 解析、网格、历史、主题和状态迁移 |
| `plugins/terminal/src/emulator.rs` | 对接上游 VT100 核心的插件适配：选择复制、查询响应、颜色和快照 |
| `plugins/example` | 不申请系统权限的计数器与笔记，用于验证宿主的通用性 |

主程序把公开接口编入 `editor-app.exe`。独立插件项目声明带 `guest` feature 的 `plugin-protocol` 版本依赖，并通过 `editor-app.exe --plugin-cargo <插件/Cargo.toml> build --target wasm32-wasip2 --release` 编译。编辑器按接口内容摘要管理用户缓存，并为本次 Cargo 调用注入依赖路径；Rust 消息类型和 WIT 绑定都来自这份缓存。项目不需要 `plugins/sdk`，发行目录不再附带 `sdk/`，插件也不引用 `crates/` 源码。相同入口支持 `check` 和 `test`；开发机需要 Rust/Cargo，使用插件的用户不需要。安装包只包含编译后的 `.wasm`、清单和资源。运行时主程序实现 WIT 的 `host.request` 导入。显式 `--export-plugin-sdk` 仅供其他工具链或接口检查使用。

主程序不识别终端命令、ANSI、光标模式、Shell 类型或终端 Tab。它绘制插件描述的矩形、文本及标准按钮/输入控件，并把原生事件连同面板 ID 发回插件。新增功能无需修改主程序中的枚举或终端专用槽位。

主题通过通用 `Environment` 传给插件：背景、文字、弱化文字、边框、强调色、选区、明暗模式、UI/等宽字体，以及主题文件 `themes[].plugins` 中按插件 ID 分组的命名颜色和文字样式。插件用 `Environment::color(plugin_id, role)`、`Environment::font_style(plugin_id, role, monospace)` 读取本插件样式；宿主在主题变化时发送 `Event::Theme(Environment)`，插件应保存新环境并重新生成所有界面。主题包不需要直接调用插件。当前主题对同名颜色和字体属性的优先级高于插件私有配置。终端配置放在 `plugins["terminal"]`，全部角色和优先级见[终端插件说明](../plugins/terminal/README.md)。

插件绘制的 `Paint::Text` 可逐项指定字体，`Scene` 提供默认字体；原生 `Widget` 的可选 `style` 包含字体、文字、背景以及悬停和按下背景。插件从当前环境解析这些值后随场景交给宿主，主题变化时重新发布场景。示例插件演示了 `example.body.foreground`、`example.control.*` 和 `example.typography.body`。旧插件未提供这些字段时继续使用宿主通用主题。

## 契约与权限

WIT 世界为 `editor:plugin/plugin@0.1.0`，只有宿主 `request` 导入与插件 `dispatch` 导出。负载类型由 `plugin-protocol` 定义；清单 `protocol=1` 使用画布消息，`protocol=2` 在相同 WIT 传输上增加原生界面树与类型化 UI 事件。WASI 不继承宿主目录、环境变量、标准输入输出或网络权限。包内资源通过只读 `ReadAsset` 访问。

插件本身不能直接读取任意本机文件，包括编辑器的接口缓存。`ReadAsset` 仅访问该插件自己的安装资源；`ReadData/WriteData` 需要 `storage` 授权且限定在该插件的私有数据目录；访问工作区文件必须通过获授 `workspace.read` 的编辑器接口。所有这些文件接口拒绝绝对路径、目录穿越、Windows 设备路径和备用数据流，并检查解析符号链接后的实际位置，不能借此访问其他插件或目录之外的文件。限制针对已安装插件的运行时；开发者主动运行 Cargo 编译源码属于本机开发操作。

| 能力 | 授权后允许 |
| --- | --- |
| `process.pty` | 创建、输入、调整和关闭本插件的 PTY 程序；不能操作其他插件句柄 |
| `workspace.read` | 读取当前工作区内的受限文件；拒绝绝对路径、目录穿越与符号链接逃逸 |
| `storage` | 读写本插件的私有配置文件 |
| `clipboard` | 请求原生 UI 线程读写剪贴板 |
| `editor.commands` | 请求读取选区、取得当前文件目录、保存文件、打开私有配置或隐藏本插件声明的面板 |

**PTY 程序以当前用户身份运行，具备该用户的文件、进程与网络权限。** WASM 本身没有这些环境权限；安装界面对 `process.pty` 明确展示其含义。此能力适合终端等确实需要运行本机工具的插件，不等同于为子进程提供操作系统级文件沙箱。

安装界面展示全部请求权限；更新也重新展示请求，缺少授权的新增能力会在运行时被拒绝。本机未签名包可以安装。签名、市场和自动下载不属于本阶段。

调用设有指令燃料、内存、进程数量、文件大小、快照大小和绘制数量上限。原生绘制前校验坐标及控件规模。Windows 子进程归入 Job Object，停用或更新会关闭进程树。PTY 读写使用有限队列，后台计算不占用 GPUI 界面线程。

## 更新和状态恢复

宿主功能可调用 `EditorApp::invoke_plugin_command(plugin_id, command_id, arguments, window, cx)`，后台 worker 通过 `Manager::invoke_command` 进入插件串行事件循环。参数是插件自行定义的 JSON，主程序验证运行状态、清单中的命令及参数容量，不依赖终端实现。`Event::Command.arguments` 为可选字段，旧宿主和旧插件仍可使用无参数命令。调用不会自动安装、启用插件或扩大权限。

终端 0.6.0 接收 `terminal.new`、`terminal.run` 的 `name`、`cwd`、`profile` 参数；运行命令还可携带 `command`，收到类型化保存成功结果后创建独立会话并执行。普通新建统一使用工具名，传入名称时使用指定名称。详见[终端接口说明](../plugins/terminal/README.md)。

1. 读取并验证本机包，取得用户授权；版本目录按包摘要保存。
2. 向旧版请求快照。宿主只认识 `schema + data`，不解释内部字段。
3. 用新版运行 `Prepare(environment, snapshot)`；插件在这里验证、迁移和恢复自身数据，不能启动程序或产生其他宿主副作用。
4. 准备成功后原子保存快照，停止旧版拥有的程序，再调用新版 `Activate`。
5. 启用成功才提交安装记录。准备失败时旧版继续运行；切换后失败则加载旧版及原快照。已停止的程序无法恢复进程内存，旧版会重新启动程序。

终端快照保留 Tab 名称、顺序、当前 Tab、Shell 配置、工作目录、主题和有容量上限的带颜色旧输出。还原时旧输出只送入 VT 解析器，**不送入 Shell 输入**；仍在运行的逻辑会话启动全新 Shell，不自动重跑原命令或还原临时变量，已退出会话只恢复历史。PowerShell 默认配置通过插件内的 Shell Integration 跟踪 `cd` 后的目录；自定义命令启动模式与未提供目录元数据的 Shell 保留其启动目录。

快照按工作区隔离，每三秒检查保存一次，正常退出时等待最后一次保存完成。崩溃时使用最后一次成功的快照。终端历史受 `history` 行数与总输出字节限制约束，超额舍弃最旧内容。宿主以临时文件和原子替换保存数据。失败或恶意插件仍可停用和卸载；取快照失败则保留上次有效数据。

默认安装目录为 `%APPDATA%/MeEditor/runtime-plugins`。`ME_EDITOR_PLUGIN_HOME` 可指定隔离的测试或便携目录。`packages/` 保存版本代码，`data/<id>/` 保存配置及各工作区快照，`registry.json` 记录当前版本、授权与启停状态。

插件 ID 为 `terminal`、`example`、`svg`、`rust`、`toml`、`html`、`javascript`，不再带 `me.` 前缀。SVG 名称为 `SVG`，源码目录为 `plugins/svg`，安装包为 `svg.zip`。新版主程序首次读取旧安装记录时迁移 ID，并复制包、配置及快照到新目录；权限、全局启停和项目启用状态保持不变。旧目录及 `registry.before-plugin-id-rename.json` 作为可恢复的备份保留。工作区的面板显示状态和 Dock 布局也随 ID 迁移。

## 验证

```powershell
# 核心行为与 GPUI 集成测试。
cargo test -p plugin-runtime --lib
./target/release/editor-app.exe --plugin-cargo plugins/terminal/Cargo.toml test --lib
cargo test -p editor-app --bin editor-app
# 必须先构建插件包，再测试实际组件与真实 Windows ConPTY。
./scripts/build-plugins.ps1
cargo run -p plugin-runtime --example smoke -- dist/plugins/terminal.zip
cargo check --workspace
# 验证真实原生窗口加载插件及正常关闭，使用 target/ 下的独立测试数据。
./scripts/runtime-startup-smoke.ps1
```

真实组件验证覆盖：权限缺失拒绝、执行命令得到输出、失败更新保留原进程、成功更新更换进程、旧输出恢复、跨运行时重启恢复、卸载保留/删除数据、非终端包的两个面板及新增权限拒绝。GPUI 测试覆盖运行中注册/移除面板、快捷键隔离、中文提交与 Enter 提交编辑。

当前已在 Windows 上验证；其他系统的原生 PTY 路径尚未做实际运行验证。图片协议、kitty 扩展键盘协议、在线市场和现有语言/主题插件迁移不包含在此次终端先行版本中。

## 终端公开能力与上游内核（0.6.0）

终端包继续编译为 `wasm32-wasip2`，通过宿主导出的 SDK 独立构建。`Cargo.lock` 固定上游 `term-wm-vt100` 的 Git 修订与依赖，移除本地 Alacritty vendor。安装/更新 `dist/plugins/terminal.zip` 即可；协议 7 能力协商先于激活。旧版 schema 1 的带颜色输出快照继续可读，schema 2 额外保存已退出状态；快照还原不向 Shell 重放输入。

上游核心管理字符样式、屏幕、滚动历史、换行重排和模式；插件适配保留中文宽字符、真彩色、选择复制、应用方向键、括号粘贴、鼠标报告、焦点报告及常用状态/颜色查询。历史配置限制立即约束滚动与快照。进程、剪贴板、保存与打开私有设置文件均通过类型化公开能力；配置省略 Shell 时按宿主操作系统补默认值。

上游项目：[term-wm-vt100](https://github.com/jzombie/term-wm-vt100)、[vte](https://github.com/alacritty/vte)。许可位于 `THIRD_PARTY_LICENSES/term-wm-vt100-LICENSE` 和 `THIRD_PARTY_LICENSES/vte-LICENSE-APACHE`，随插件包分发。验收见[终端迁移记录](specs/plugin-api-terminal-migration-verification.md)。
