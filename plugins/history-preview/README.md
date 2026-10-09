# History Preview

This independent SDK example displays fixed historical text in a readonly native document.
Open a local text document, then use **Compare historical content** from the plugin command
menu. The left pane shows history; the right pane keeps the current local session, including
unsaved edits. **Refresh historical content** refreshes the readonly resource and compares
its new exact version. A source edit closes the comparison; invoke the command again to
compare the latest text. Close comparison returns keyboard focus to the right document.

The historical document rejects editing and saving. Left-focused Save does not save a dirty
right document. Disabling or replacing this plugin removes its resources and comparisons;
local documents remain open.

Build from the repository root with the built host:

```powershell
& ./target/debug/editor-app.exe --plugin-package plugins/history-preview --output dist/plugins --debug
```

The package entry calls the host's `--plugin-cargo` workflow, which supplies the exported SDK
cache. This project has no dependency on a repository crate path. It requires wire protocol
7, `editor.documents ^1.1`, `editor.virtual ^1`, `editor.diff ^1` and `ui.native ^1`, with
`editor.read` and `ui.panels` permissions.

本独立示例显示固定的历史文本。打开本地文本后，从插件命令菜单执行 **Compare historical content**；
左侧历史只读，右侧保留当前会话的未保存内容。**Refresh historical content** 刷新只读版本并重新比较。
任一源编辑后比较关闭，需要再次执行命令；关闭比较将焦点返回右文档。左侧输入和保存被拒绝，
不会误保存右侧脏文档；禁用或替换插件清理它拥有的视图和资源，保留本地文档。
