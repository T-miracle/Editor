# Me Editor

Me Editor 是一个 Rust + GPUI Kit 编写的原生桌面代码编辑器初版。

## 当前可用功能

- 原生三段式界面：文件区、代码编辑区、底部输出区。
- 递归文件树，并遵循 `.gitignore`。
- JetBrains 2023 风格深浅色主题，以及按文件类型显示的彩色 SVG 图标。
- 打开 UTF-8 文件并按扩展名启用语法高亮。
- 行号、缩进参考线、代码折叠和软换行切换。
- 编辑脏状态、保存按钮与 `Ctrl+S`。
- 保存前在用户本地数据目录创建历史快照，不污染项目目录。
- 声明式插件清单解析和校验骨架。
- 统一命令注册表骨架。

## 运行

已构建的 Windows 可执行文件可直接运行：

```powershell
.\dist\me-editor.exe
```

从源码运行：

日常使用建议采用 Release 构建；`cargo run` 默认生成未优化的 Debug 程序，窗口拖动可能明显卡顿。

```powershell
cd C:\Project\Me\me-editor
cargo run --release -p editor-app
```

也可以指定要打开的工作区或文件：

```powershell
cargo run --release -p editor-app -- C:\path\to\project
cargo run --release -p editor-app -- C:\path\to\project\src\main.rs
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

当前版本优先证明编辑闭环可运行。动态 WASM grammar、外部 LSP、Git 写操作、搜索替换预览和 SQLite 历史索引将在后续版本接入现有模块接缝。

## 第三方资源

文件图标来自 [peakoss/vscode-jetbrains-icon-theme](https://github.com/peakoss/vscode-jetbrains-icon-theme/tree/main/assets/2023)，按 MIT 许可证使用。完整许可文本位于 `THIRD_PARTY_LICENSES/vscode-jetbrains-icon-theme-LICENSE.md`。
