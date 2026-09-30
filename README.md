# Me Editor

Me Editor 是一个 Rust + GPUI Kit 编写的原生桌面代码编辑器初版。

## 当前可用功能

- 原生可调布局：文件区与代码编辑区。
- 递归文件树，并遵循 `.gitignore`。
- JetBrains 2023 风格深浅色主题，以及按文件类型显示的彩色 SVG 图标。
- 打开 UTF-8 文件并按扩展名启用语法高亮。
- Rust 插件提供 WASM 语法高亮和定义跳转；在符号处按 `F12` 或点击鼠标中键可打开项目文件、Cargo 依赖及已安装的标准库源码。中键点击的位置没有定义时，会短暂显示“暂无定义”。
- Rust 项目内的 TOML 文件使用 Rust 配置文件图标。
- 行号、缩进参考线、代码折叠和软换行切换。
- 插件 WASM 语法诊断：Rust、TOML 语法错误显示红色波浪线，悬浮查看说明；状态栏显示当前文件的错误数量，点击或按 `F8` 跳转到下一处，`Shift+F8` 返回上一处。编辑修正后自动清除错误。
- LSP 语义诊断：编辑停顿 180 ms 后同步未保存内容，支持标准诊断请求和服务器推送，显示类型错误、未定义函数和警告，并在悬浮卡片中保留来源、错误码和完整说明。Rust 使用已安装的 rust-analyzer，开启原生及实验性诊断以支持输入时检查（实验性检查可能误报）；原生分析尚未覆盖的问题（例如部分未定义类型）由保存后的编译检查补充。`F8`／`Shift+F8` 同样可以跳转这些问题。
- 编辑脏状态、保存按钮与 `Ctrl+S`。
- 保存前在用户本地数据目录创建历史快照，不污染项目目录。
- 声明式插件清单解析和校验骨架。
- 统一命令注册表骨架。
- WebAssembly 运行时插件：本机安装、热更新、权限确认、状态保存与通用 GPUI 停靠界面。终端插件内的 Alacritty 核心、右侧 Tab 与交互均在插件内，详见 [运行时插件平台](docs/runtime-plugins.md) 和 [终端插件使用说明](plugins/terminal/README.md)。

## 运行

已构建的 Windows 可执行文件可直接运行：

```powershell
.\dist\me-editor.exe
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
cargo run --release -p editor-app
```

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
