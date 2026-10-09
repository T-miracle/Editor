# 示例 WebAssembly 插件

0.4.0 增加“分步骤可取消检查”原生命令。可从编辑器、实际选区、文件树或 Tab 菜单运行，按快选 → 中文名称输入 → 确认进入有来源、可取消的进度。点击进度取消不会被显示为成功；从命令菜单运行“完成检查”结束当前进度。该流程只使用公开 `ui.interaction`，业务步骤和结果保留在插件中。

0.4.0 使用协议 7 的公开 SDK 与 `ui.native` 能力，演示行列布局、按钮、输入框、复选框、单选组、标签页、列表、表格、进度条、滚动条和模态弹窗。布局定义在 `src/views.rs`，业务事件在 `src/lib.rs`。笔记输入框由主程序保留 IME 与编辑状态，颜色和字体继承当前主题。SDK 导出目录的 `UI.md` 包含完整契约。

本插件申请 `ui.interaction` 以使用宿主快选、输入、确认和进度，不申请原生进程或任意文件权限：计数与笔记通过宿主管理的逻辑快照保存，启停和重装保留数据；不申请进程、剪贴板、工作区或私有文件权限。同帧连续文本输入保持稳定的 UI 版本，切换标签与开关模态弹窗才更新交互目标版本。安装即生效，禁用或卸载撤销两个面板。

本目录包含计数器与笔记插件的完整实现：`src/` 为代码，`Cargo.toml` 为 WASM crate，`manifest.json` 声明面板与命令。编译接口由编辑器自动缓存和注入，不需要同级 `sdk/` 或主程序源码。独立构建使用 `editor-app.exe --plugin-cargo example/Cargo.toml build --target wasm32-wasip2 --release`。

通过宿主 `editor-app.exe --plugin-package plugins/example` 完整构建并封装 ZIP，默认输出为插件项目根部的 `example-0.4.0.zip`。项目描述在 `nanobug-plugin.json`，也可使用宿主“插件打包”配置选择输出位置；不调用归档脚本。
