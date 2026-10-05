# Run target example / 运行目标示例

This resource-only package contributes `native-tool` targets from `*/native-tool.toml`. It needs no
component, native dependency, or private host interface. Discovery reads declarative values only;
confirming a candidate creates an editable configuration. Executing it still requires a trusted
workspace and an authorized execution provider.

此纯资源包从 `*/native-tool.toml` 贡献 `native-tool` 候选。发现只读取声明，确认后才保存配置；
执行仍须受信任工作区及已授权执行提供者。与 Rust 提供者共用 B1 表单和顶栏入口。

```toml
[tool]
name = "Example program"
program = "my-program.exe"
# One literal argument per line; this is never interpreted as a shell command.
arguments = "--verbose\n中文参数"
```
