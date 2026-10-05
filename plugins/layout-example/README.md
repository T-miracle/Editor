# 可组合文件布局示例

独立插件包通过公开 `editor.layout` 引用当前文本编辑器，提供并排、上下和仅插件内容三种布局。
编辑器文本、选区与撤销仍由宿主原生会话拥有；插件只记布局。PNG 使用独立文件上下文，不创建编辑器。
同一个组件可更换清单 ID 安装为另一提供者，用文件标签右键菜单明确选择。
底栏工具及图标由插件包提供；显示选择通过 `storage.private 1.1` 按工作区和文件类型保存，
使用 CAS 与可撤销订阅保持实例一致，不复制文本或 Undo 状态到插件。

```powershell
cargo build -p editor-app
./scripts/build-layout-example.ps1 -HostExe ./target/debug/editor-app.exe
```

构建产物在 `target/plugin-layout-test/`，不作为内置产品插件发行。
