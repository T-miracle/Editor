# 独立窗口与辅助工具示例

使用公开 SDK 贡献五个独立窗口及当前文件的辅助工具，不替换所选文件布局。
工具展示双语提示、选中、禁用和隐藏状态；计数操作由插件处理，可用来检查焦点和溢出菜单的目标。

```powershell
cargo build -p editor-app
./target/debug/editor-app.exe --plugin-package plugins/tools-example
```

宿主按 `nanobug-plugin.json` 构建组件并收集清单、README、tools-example.wasm 与 icons，自动生成项目根部的 `tools-example-0.1.1.zip`。需要旧测试夹具路径时将 ZIP 复制为 `target/plugin-layout-test/tools-example.zip`；本示例不作为内置产品插件发行。
