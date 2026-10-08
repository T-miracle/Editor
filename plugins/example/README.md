# 示例 WebAssembly 插件

0.3.1 使用协议 7 的公开 SDK 与 `ui.native` 能力，演示行列布局、按钮、输入框、复选框、单选组、标签页、列表、表格、进度条、滚动条和模态弹窗。布局定义在 `src/views.rs`，业务事件在 `src/lib.rs`。笔记输入框由主程序保留 IME 与编辑状态，颜色和字体继承当前主题。SDK 导出目录的 `UI.md` 包含完整契约。

本插件无需任何原生权限：计数与笔记通过宿主管理的逻辑快照保存，启停和重装保留数据；不申请进程、剪贴板、工作区或私有文件权限。同帧连续文本输入保持稳定的 UI 版本，切换标签与开关模态弹窗才更新交互目标版本。安装即生效，禁用或卸载撤销两个面板。

本目录包含计数器与笔记插件的完整实现：`src/` 为代码，`Cargo.toml` 为 WASM crate，`manifest.json` 声明面板与命令。编译接口由编辑器自动缓存和注入，不需要同级 `sdk/` 或主程序源码。独立构建使用 `editor-app.exe --plugin-cargo example/Cargo.toml build --target wasm32-wasip2 --release`。

通过实际宿主 `--plugin-cargo plugins/example/Cargo.toml build --target wasm32-wasip2 --release` 独立构建组件，再用 ZIP 工具归档清单、README 与 example.wasm。步骤见[直接打包说明](../../installer/README.md)，通过插件管理界面安装。Cargo 包名仍为 example-guest。
