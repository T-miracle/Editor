# Me Editor

Me Editor 是一个 Rust + GPUI Kit 编写的原生桌面代码编辑器初版。

## 当前可用功能

- 原生可调布局：文件区与代码编辑区。
- 递归文件树，并遵循 `.gitignore`。
- JetBrains 2023 风格深浅色主题，以及按文件类型显示的彩色 SVG 图标。
- 打开 UTF-8 文件并按扩展名启用语法高亮。
- Rust 插件提供 WASM 语法高亮和定义跳转；在符号处按 `F12` 或点击鼠标中键可打开项目文件、Cargo 依赖及已安装的标准库源码。中键点击的位置没有定义时，会短暂显示“暂无定义”。
- Rust 项目内的 TOML 文件使用 Rust 配置文件图标。
- JavaScript 插件为 `.js`、`.mjs`、`.cjs`、`.jsx` 提供 WASM 语法高亮、语法错误提示和深浅色图标，无需 Node.js；详见 [JavaScript 插件说明](plugins/javascript/README.md)。
- HTML 插件为 `.html`、`.htm` 提供 WASM 解析、标签与属性高亮和深浅色文件图标；安装方式及范围见 [HTML 插件说明](plugins/html/README.md)。
- 行号、缩进参考线、代码折叠和软换行切换。
- 选中文本后按住鼠标左键拖动，松开后移动到落点；支持边缘自动滚动、`Esc` 取消和一次撤销恢复。
- 插件 WASM 语法诊断：Rust、TOML、HTML、JavaScript 解析器报告的语法错误显示红色波浪线，悬浮查看说明；状态栏显示当前文件的错误数量，点击或按 `F8` 跳转到下一处，`Shift+F8` 返回上一处。编辑修正后自动清除错误。
- LSP 语义诊断：编辑停顿 180 ms 后同步未保存内容，支持标准诊断请求和服务器推送，显示类型错误、未定义函数和警告，并在悬浮卡片中保留来源、错误码和完整说明。Rust 使用已安装的 rust-analyzer，开启原生及实验性诊断以支持输入时检查（实验性检查可能误报）；原生分析尚未覆盖的问题（例如部分未定义类型）由保存后的编译检查补充。`F8`／`Shift+F8` 同样可以跳转这些问题。
- 编辑脏状态、保存按钮与 `Ctrl+S`。
- SVG 插件：左侧编辑源码、右侧实时预览，支持拖动分割线、透明棋盘底座及始终居中的滚轮缩放；详见 [SVG 插件说明](plugins/svg/README.md)。
- Markdown 插件随发行包交付，提供源码高亮、顶部格式工具栏、三种视图模式和双向内容块同步滚动；粘贴或拖入图片保存到文件同级目录。受信任工作区首次使用需确认权限，并保留拒绝、禁用、卸载及替代提供者选择；详见 [Markdown 使用说明](plugins/markdown/README.md)。
- 保存前在用户本地数据目录创建历史快照，不污染项目目录。
- 声明式插件清单解析和校验骨架。
- 统一命令注册表骨架。
- WebAssembly 运行时插件：本机安装、热更新、权限确认、状态保存与通用 GPUI 停靠界面。终端插件内的 Alacritty 核心、右侧 Tab 与交互均在插件内，详见 [运行时插件平台](docs/runtime-plugins.md) 和 [终端插件使用说明](plugins/terminal/README.md)。
- 运行、调试与构建：标题栏运行组配置并启动真实程序、单独构建、观察并行会话；程序与调试均由插件提供者经公开契约承担，宿主不内建语言或调试器知识。断点属于配置，断点命中时自动定位到源码行，暂停后可查看调用栈与局部变量。使用说明见 [运行、调试与构建](https://t-miracle.github.io/Editor/zh-cn/guide/run-debug-build/)，公开协议见 [SDK 文档](https://t-miracle.github.io/Editor/zh-cn/sdk/sessions/)。维护者验收保存在 [验证记录](docs/plugins/verification/run-debug-build.md)。

## 运行

已构建的 Windows 可执行文件可直接运行：

```powershell
.\dist\editor\editor-app.exe
```

从源码以 Debug 开发模式运行（不打包），`cargo run` 默认生成未优化的 Debug 程序，窗口拖动可能明显卡顿。：

```powershell
cd C:\Projects\RustProjects\Editor
cargo run -p editor-app -- .
```

也可以指定要打开的工作区或文件：

```powershell
cargo run -p editor-app -- C:\path\to\project
cargo run -p editor-app -- C:\path\to\project\src\main.rs
```

打包为正式包：

```powershell
# 构建正式编辑器、内置插件包及首次提供索引。
.\scripts\package-editor.ps1
```

产物位于 `dist/editor/`，插件位于其 `plugins/` 子目录。Markdown 方案、工单与逐单验收见[插件文档入口](docs/plugins/README.md)。

## 运行与调试插件构建

运行准备和调试均使用独立插件。已有 Rust/Cargo 与 `wasm32-wasip2` 目标的开发环境可执行：

```powershell
cargo build -p editor-app
./scripts/build-plugins.ps1 -HostExe ./target/debug/editor-app.exe -Packages terminal,rust,run-target-example
./scripts/build-rust-debugger.ps1 -HostExe ./target/debug/editor-app.exe
./scripts/verify-plugin-sdk.ps1
```

Rust 调试插件将经哈希锁定的 CodeLLDB 与桥接程序准备到已授权的插件私有目录，不修改全局工具路径或安装编译器。安装包仍需正常批准声明的权限。读者文档源在 `website/`，站点构建说明见 [website/README.md](website/README.md)。

## 验证

```powershell
cargo fmt --check
cargo test --workspace --exclude editor-app
cargo check --workspace
```

## 工作区结构

```text
crates/
├─ editor-app/        GPUI Kit 窗口、编辑器和交互
├─ editor-core/       文档、工作区与命令模块
├─ platform-windows/  原子保存和本地历史适配器
└─ plugin-schema/     插件清单稳定格式与校验
```

Rust 跳转使用插件清单声明的 `rust-analyzer`。编辑器会查找已安装的语言服务，依赖和标准库源码需要在本机可用。Git 写操作、搜索替换预览和 SQLite 历史索引仍在后续版本接入现有模块接缝。

## 第三方资源

GPUI 界面依赖使用 crates.io 发布的上游版本，由 `Cargo.lock` 锁定；项目不使用 `gpui-base` 或 `gpui-component` 的本地源码覆盖。插件停靠区的显隐适配保留在应用层，补全与悬浮提示采用上游组件行为。

文件图标由当前插件资源提供。

终端 WASM 插件使用内置的 Alacritty 0.26.0 WASM 适配核心和上游 `vte`；详见 [运行时插件文档](docs/runtime-plugins.md)。
