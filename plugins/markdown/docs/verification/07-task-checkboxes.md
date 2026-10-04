# 07 — 预览任务框验证记录

状态：本单功能、回归、门禁与审查通过。基线 `1a8e65fb4350e7cff6ff46de9dd823d28da690e4`。

2026-10-04 已读回 [#33](https://github.com/T-miracle/Editor/issues/33) open / ready-for-agent 及直接前置 #28、#30 closed / completed。上一单 #32 已推送并读回关闭。

主接缝：实际独立 Markdown ZIP → 公开 Manager → 原生任务框 pointer／keyboard → 版本化编辑事务 → 源码、可见勾选与原生 Undo／Redo。复用 `ui.native` 的 Checkbox／Toggle 和 `editor.edit`，不引入第二份任务状态或预览编辑器；派生源码范围用于定位单个 `[ ]`／`[x]` 标记。测试包含嵌套中文、过期事件、切换／关闭、停用及原生输入边界。实际 RED／GREEN、命令与审查结果在完成后补录。

首个纵向回归：`cargo test -p editor-app delivered_markdown_task_click -- --ignored --nocapture --test-threads=1` 初轮 0 passed / 1 failed（`target/07-native-task-red.log`）。实现首切片后仍失败，实际 public Node 范围 `2..5` 正确且已启用；诊断发现嵌套列表把外层 wrapper 拉成 25 × 53 px，中心落在真实 16 px 标记之外（`target/07-native-task-diagnostic.log`）。这是测试命中位置问题，不将该失败归因于产品范围解析。

改用通用原生标记的 debug selector 后，以已提交 `1a8e65f` 的实际 `0.6.0` 源码在仓库外通过 SDK 重建旧包，正确位置点击实际 RED，0 passed / 1 failed，19.65 秒（`target/07-native-task-corrected-red.log`）；恢复 `0.7.0` 实际 ZIP 后同一用例 GREEN，1 passed / 0 ignored，22.35 秒（`target/07-native-task-corrected-green.log`）。源字符、选区、磁盘未保存、可见任务状态及一次 Undo／Redo 均通过。最初把旧源码放到 workspace 的 target 子目录导致 Cargo workspace 归属拒绝，改为系统临时目录后构建通过；不计作行为 RED。未改写历史或修改旧包源码。

通用原生 Checkbox 使用已有节点 tooltip 作为空可见 label 的无障碍名称，hover 与键盘焦点由 gpui-base 接缝及本地外观提供；没有新公开字段或 Markdown 业务分支。既有 `cargo test -p editor-app native_clicks_emit_typed_events -- --nocapture` 1 passed（`target/07-native-checkbox-controls.log`），真实指针点击和 disabled 无效行为保留。

## 键盘、版本与解析边界

- `delivered_markdown_nested_tasks` 的真实仅预览 Space 回归先失败，0 passed / 1 failed，20.91 秒（`target/07-native-nested-keyboard.log`）；`delivered_markdown_split_task` 先在任务点击后的源码焦点断言失败，0 passed / 1 failed，20.10 秒（`target/07-native-split-focus-red.log`）。原因分别是发布空隙退休控件与通用范围编辑强制移回源码。修复保留同 source ID/path 的原生 entity、显式 Checkbox FocusHandle，并只在没有预览控件焦点时把编辑焦点交回源码；toolbar 仍为独立投影。旧版本树保持隐藏，跨文档、停用及关闭照常退休。上述两个原生回归随后分别 1 passed，21.89／21.23 秒（`target/07-preview-keyboard-green.log`、`target/07-split-focus-green.log`）。
- Standards 审查指出 Base 的 pending mouse down 随元素 ID 留存。新增 `checkbox_press_cannot_activate_a_replacement_scene`，真实按下 → 同节点 ID 新 revision → draw → 释放，先 0 passed / 1 failed，随后 1 passed，0.02 秒（`target/07-stale-gesture-{red,green}.log`）。手势 ID 绑定 UI revision，事件携带绘制时 revision；焦点身份独立，避免丢失后续键盘操作。
- Spec 审查发现 pulldown-cmark 接受 TAB、VT、FF 空白任务标记。真实 `delivered_markdown_parser_blank` 点击 TAB 先 0 passed / 1 failed，19.43 秒（`target/07-parser-blank-red.log`），然后在最终真实 ZIP 组中通过。仅接受解析节点的精确三字节标记，保存原空白字符再核对；Undo 恢复原字符。换行、Unicode 空格、双空格及拆开 UTF-8 的选区保持只读／拒绝。

## 最终实际包回归

Windows，Markdown `0.7.0`、protocol 7；复用现有 `ui.native`、`ui.richtext`、`editor.edit`，没有新增 SDK 字段或能力版本。

- `target/debug/editor-app.exe --plugin-cargo plugins/markdown/Cargo.toml test`：31 passed / 0 ignored（`target/07-guest-final.log`）。
- `./scripts/build-plugins.ps1 -HostExe target/debug/editor-app.exe -Packages markdown`：独立 SDK 构建及实际 ZIP 通过，9.15 秒（`target/07-package-final.log`）。先前旧包 RED 使用的旧组件已被本次当前源码构建替换。
- `cargo test -p editor-app extensions::markdown_tests::task_ -- --ignored --nocapture --test-threads=1`：6 passed / 0 ignored，130.54 秒（`target/07-task-native-final.log`）。包含中文／嵌套／大小写标记、Tab 字符、实际可见勾选、磁盘仍为原文、原选区、单次 Undo／Redo、分栏与仅预览 Space、深浅主题 16 px 标记，以及快速原生输入、旧 UI revision 拒绝、中文 marked composition 保留、切换／关闭／停用后的真实请求取消。非任务文字与代码中的 `[ ]` 不变成可编辑控件。
- `cargo test -p editor-app ui::plugin::tests -- --nocapture --test-threads=1`：13 passed / 0 ignored，0.12 秒（`target/07-ui-controls-final.log`），含真实 Base 控件、焦点、模态、原生集合与旧手势门禁。

范围编辑由原生 Change 更新唯一 DocumentSession；访客共享格式意图控制器完成读取、版本／选区核对、单字节写入与取消。新源码到达后重解析，不维护第二份任务勾选状态或撤销历史。后续 08–10 的导航、高亮与同步滚动尚未在本单宣称完成。

## 门禁与审查

`cargo fmt --check`、访客 `cargo fmt --manifest-path plugins/markdown/Cargo.toml --check`、`cargo check --workspace` 均通过（`target/07-fmt-check.log`、`target/07-workspace-check.log`）。`cargo test --workspace --exclude editor-app` 实际 67 passed / 93 ignored，35 个结果组（`target/07-workspace-test.log`）；93 个跳过用例没有计为通过。本单实际 WASM／原生用例已按上述命令显式运行。

`cargo test -p editor-app delivered_markdown_toolbar -- --ignored --nocapture --test-threads=1`：既有两条真实工具栏回归 2 passed / 0 ignored，61.97 秒（`target/07-toolbar-regression.log`），格式完成仍回源码，模板输入、全部按钮、Undo／Redo 和停用行为保留。包目录 Markdown 本地链接与 `git diff --check` 通过。保留环境既有 LNK4217 与未使用 API 警告。

独立 Standards 复审：硬性违规 0、Fowler 判断性发现 0；独立 Spec 复审：0 findings。审查基线固定为 `1a8e65f`；依已授权“测试与审查 → 提交”顺序使用 `git diff <base> -- crates/editor-app plugins/markdown` 与新增文件，`git log <base>..HEAD` 为空，如实审查工作树而非宣称提交差异。已修复初审的版本手势、原生焦点和非空格任务标记问题，并各自保存真实 RED／GREEN。

#26 保持原状态；提交／推送／关闭读回后续追加，不改写已推送历史。SDK 公共契约在本单未变化，使用 06 已验证的摘要 `592bd51824e94f5a79f06eb75664ad7f9cb27033bee7786c265e86a3c682a988` 独立构建当前包；没有因无关 SDK 变更追加验证。

2026-10-04 交付：普通提交 `24a27dc8bbfc2cb16557fc7d28f22bbafa2872ed` 已推送 `origin/codex/markdown-plugin`，`git ls-remote` 的远端 SHA 一致；GitHub #33 PATCH 后独立 GET 读回 `closed / completed`。此读回追加于下一工单，不改写已推送历史。
