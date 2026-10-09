# Generated Preview

This independent SDK example reads the current unsaved text and derives an uppercase
proposal. Open a local text document, then use **Preview generated text** from the plugin
command menu. A readonly left document shows the proposal beside the current local session.
Run the command again to refresh and compare new exact versions. A source edit closes the
comparison; the plugin never applies its proposal to the writable document. Close comparison
returns keyboard focus to the right document.

The generated document rejects editing and saving. Left-focused Save does not save a dirty
right document. Disabling or replacing this plugin removes its resources and comparisons;
local documents remain open.

Build from the repository root with the built host:

```powershell
& ./target/debug/editor-app.exe --plugin-package plugins/generated-preview --output dist/plugins --debug
```

The package entry calls the host's `--plugin-cargo` workflow and exported SDK cache. No
repository crate path is required. The package declares wire protocol 7,
`editor.documents ^1.1`, `editor.virtual ^1`, `editor.diff ^1` and `ui.native ^1`, plus
`editor.read`, `workspace.read` and `ui.panels`. The additional filesystem permission allows
its generic local document opener; the proposal itself has no backing file.

本独立示例读取当前未保存文本并派生大写建议。打开本地文本后，在插件命令菜单执行
**Preview generated text**；左侧建议只读，右侧仍是当前本地会话。再次执行可刷新新版本并重新比较，
源编辑会关闭旧比较；插件不会把建议写回右侧。关闭比较将焦点返回右文档。左侧输入和保存被拒绝，
不会误保存右侧脏文档；禁用或替换插件清理它拥有的视图和资源，保留本地文档。
