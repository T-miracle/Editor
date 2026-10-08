# 本地 main 合并与原有改动整合

日期：2026-10-06，Windows。用户授权合并、解决冲突、启动查看；明确暂不推送远程。

后续：用户再次从默认 release 目录启动后报告访问拒绝；原目录继承标签已另行恢复，并验证新 EXE 与默认 Cargo release 启动。见[复发验收](../plugin-manager-access-recurrence.md)，下文保留合并当时使用独立构建目录的证据与限制。

## Git 与保留范围

- 插件分支 `codex/plugin-ui-decoupling` 的交付终点为 `33c5bc1060f877d3e892b9785b388c6b5517a9f7`。
- 本地合并提交 `db53d2d0bcaae7ca3106f83227e8d1ef902215af` 的两个父提交是 `0ed129d` 与 `33c5bc1`，本地 `main` 已快进到该合并提交。没有执行 push、修改父议题或重复关闭工单。
- 合并前 `main` 为 `4d4a7b6`，包含大量未提交代码、文档搬迁和未跟踪文件。完整原始内容保存在 stash `989d5b20351242e110e0a58ce319e1d03c91827a`，更早的 stash `2b6f63cd8ab1f0c399b8e9aee0290776a9cfe9f9` 同样保留。
- 恢复原有内容后解决 26 项冲突；暂存区恢复为空，原有改动及必要的接口适配留在工作区，未混入合并提交。当前不存在未解决冲突。

## 整合内容

保留原工作区的 Markdown 增量解析、原生图标、预览虚拟化、输入发布合并及运行配置改动，适配当前公开协议。Markdown 包提升至 0.16.0，显示模式、工具栏开关、图标和内容配色继续由插件提供；宿主没有恢复旧 `editor.presentation` 或宿主工具栏开关。

源文件通知与文本预览成对发布，权限判断始终使用当前文档版本，过期树只用于绘制。格式按钮会先提交合并中的最新源文本；同一文件的新 revision 保留最后手动滚动的一侧。SDK 导出支持站点文档的 LF 与 CRLF frontmatter，正文不被截断。

## 已执行验证

以下是恢复原有改动后的当前工作区结果，不替代 [分支最终验收](05-integration-and-contract.md)。ignored 数量只记录为跳过。

| 检查 | 实际结果 |
| --- | --- |
| `cargo fmt --check`、独立 Markdown 格式检查、`git diff --check` | 最终均通过；独立插件初次检查的格式差异已修正 |
| `cargo test --workspace --exclude editor-app` | 134 passed、125 ignored，0 failed；为测试单独设置可写 TEMP/TMP |
| `cargo check --workspace`、`cargo build -p editor-app` | 通过 |
| Markdown 公开 SDK 单元测试 | 最终 71 passed、0 failed、0 ignored，包含最新滚动适配的源码 |
| 完整应用串行测试，原 main 输出目录 | 248 passed、10 failed、125 ignored；10 项失败均涉及工作区状态持久化，见下文 |
| 真实 Markdown 包原生交互 | 4 项通过：底栏图标及工具栏显隐、输入后立即格式化、源码区域高度、待处理编辑期间预览滚动；最后一项修正后重跑通过 |
| `plugin-runtime` 的 `incremental_ui --ignored` | 2 passed，0 ignored |
| 全部发行插件包 | 8 个重建通过；Markdown 0.16.0 在滚动修正后再次打包 |
| 站点检查、构建与测试 | 17 passed |
| 同一源码使用正常完整性构建目录的完整应用测试 | 258 passed、0 failed、125 ignored；上述 10 项持久化用例全部通过，SDK 缓存完整性、复用与损坏修复 3 项也通过 |
| `scripts/verify-plugin-sdk.ps1`，正常构建宿主 | 公开导出、损坏导出修复、仓库外 WASM 构建与 ZIP 分发通过；最初把 TEMP 放入另一 Cargo workspace 导致独立夹具错误，恢复系统 TEMP 后通过 |
| 新导出的实际 SDK 包经公开 Manager 安装和调用 | `sdk_distribution --ignored`：1 passed，0 ignored |
| 旧工具栏资产不能发布宿主开关 | `editor_edit legacy_toolbar_asset_cannot_publish_a_host_toggle --ignored`：1 passed，0 ignored |
| 实际启动窗口 | 同一 main 源码构建的程序 PID 18112 已启动，原生窗口显示 gear.svg 文件 Tab、源码、完整图片预览、100% 缩放及底栏模式工具按钮 |

## Windows 构建目录环境

`icacls` 读回确认原 main 的新 `target/debug/editor-app.exe` 和应用测试 EXE 均继承 `Low Mandatory Level`；独立工作区的测试 EXE 没有该标签。原 main 的 SDK 缓存写入出现 `os error 5`，状态保存测试无法读回刚保存的内容。初次 CLI 启动短暂出现窗口后已退出，Windows 自动化启动超时，这两次尝试不计为启动验收通过。

本轮在既有独立工作区的正常构建目录重新构建同一 main 源码，完整应用测试的 10 项持久化失败均消失，SDK 写入和实际启动也通过。没有修改 Windows 安全设置或文件完整性标签。原目录的 Low 标签来源尚未确定；历史同类问题见 [Windows 访问拒绝记录](../plugin-manager-access-verification.md)，历史“已修复”状态不代表本次生成文件仍正常。

验证日志位于 `%LOCALAPPDATA%/Temp/editor-main-*.log`。原 main 的 SDK 缓存曾用公开导出内容预填后完成插件构建；最终正常构建环境另外运行了 SDK 分发脚本与缓存修复测试，结果通过。

当前运行的 EXE 为 `C:/Users/Tmiracle/.codex/worktrees/merge-plugin-ui-main/Editor/target/debug/editor-app.exe`，源码和工作目录仍是本地 main。它使用 `target/merged-main-demo-20261006/plugins` 的独立演示数据，仅复用此前已授权的 Image 0.5.0 包与权限，未覆盖正常用户插件安装状态。

最终正常环境复核的命令如下，执行目录为原 main；SDK 脚本使用系统 TEMP，避免把独立夹具置于任何 Cargo workspace 内。

```powershell
# 仅将当前 main 的构建输出放入既有独立工作区，不切换源代码分支。
$mergeTarget = 'C:/Users/Tmiracle/.codex/worktrees/merge-plugin-ui-main/Editor/target'
$mergeHostPath = Join-Path $mergeTarget 'debug/editor-app.exe'
cargo build -p editor-app --target-dir $mergeTarget
cargo test -p editor-app --target-dir $mergeTarget -- --test-threads=1
./scripts/verify-plugin-sdk.ps1 -HostExe $mergeHostPath
& $mergeHostPath --plugin-cargo './plugins/markdown/Cargo.toml' test --locked
cargo test -p plugin-runtime --target-dir $mergeTarget --test sdk_distribution -- --ignored --test-threads=1
cargo test -p plugin-runtime --target-dir $mergeTarget --test editor_edit legacy_toolbar_asset_cannot_publish_a_host_toggle -- --ignored --test-threads=1
./scripts/build-plugins.ps1 -Packages markdown -HostExe $mergeHostPath
```

原 main 默认 `target/debug` 的完整性标签问题仍存在，不能把正常目录的结果视为默认 `cargo run` 已修复。本轮未重新执行 release 或全套 U01–U20；未受整合影响的插件行为继续引用分支验收，macOS/Linux 未实测。保留的原工作区 Markdown 性能问题也未因本次合并自动关闭。
