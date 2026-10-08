# 可组合文件布局示例

独立插件包通过公开 `editor.layout` 引用当前文本编辑器，提供并排、上下和仅插件内容三种布局。
编辑器文本、选区与撤销仍由宿主原生会话拥有；插件只记布局。PNG 使用独立文件上下文，不创建编辑器。
同一个组件可更换清单 ID 安装为另一提供者，用文件标签右键菜单明确选择。
底栏工具及图标由插件包提供；显示选择通过 `storage.private 1.1` 按工作区和文件类型保存，
使用 CAS 与可撤销订阅保持实例一致，不复制文本或 Undo 状态到插件。

```powershell
cargo build -p editor-app
.\target\debug\editor-app.exe --plugin-cargo plugins/layout-example/Cargo.toml build --target wasm32-wasip2 --release --target-dir target
```

组件产物位于 `target/wasm32-wasip2/release/`。按[直接归档说明](../../installer/README.md)将清单、README、layout-example.wasm 与 icons 准备为标准 ZIP，测试夹具保存到 `target/plugin-layout-test/layout-example.zip`；本示例不作为内置产品插件发行。

`diagnostic-trap` 是显式诊断命令，不贡献菜单或功能按钮；通过公开 Manager 调用它可模拟 WASM 故障，
验证文本会话、图片标签、资源撤销与手动重试。宿主无需为示例身份添加专属分支。
