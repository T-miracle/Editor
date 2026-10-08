# 独立窗口与辅助工具示例

使用公开 SDK 贡献五个独立窗口及当前文件的辅助工具，不替换所选文件布局。
工具展示双语提示、选中、禁用和隐藏状态；计数操作由插件处理，可用来检查焦点和溢出菜单的目标。

```powershell
cargo build -p editor-app
.\target\debug\editor-app.exe --plugin-cargo plugins/tools-example/Cargo.toml build --target wasm32-wasip2 --release --target-dir target
```

组件产物位于 `target/wasm32-wasip2/release/`。按[直接归档说明](../../installer/README.md)将清单、README、tools-example.wasm 与 icons 准备为标准 ZIP，测试夹具保存到 `target/plugin-layout-test/tools-example.zip`；本示例不作为内置产品插件发行。
