# 示例 WebAssembly 插件

0.2.0 使用原生界面协议 v1（清单 `protocol = 2`），演示行列布局、按钮、输入框、复选框、单选组、标签页、列表、表格、进度条、滚动条和模态弹窗。布局定义在 `src/views.rs`，业务事件在 `src/lib.rs`。笔记输入框由主程序保留 IME 与编辑状态，颜色和字体继承当前主题。SDK 导出目录的 `UI.md` 包含完整契约。

本目录包含计数器与笔记插件的完整实现：`src/` 为代码，`Cargo.toml` 为 WASM crate，`manifest.json` 声明面板与命令。编译时使用同级 `sdk/`，由已打包的主程序导出，不读取主程序源码。

在仓库根目录执行 `./scripts/build-plugins.ps1`，生成标准 ZIP 包 `dist/plugins/example.zip`，然后通过编辑器的插件管理界面安装。Cargo 包名仍为 `example-guest`。
