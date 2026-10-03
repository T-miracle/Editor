# 02 — 原生 Markdown 预览验收

日期：2026-10-04。基线：`72b23146258bf93deebe531b7b3c63eda27e4483`。包版本：`0.2.0`。状态：行为、必需检查及 Standards / Spec 双轴审查通过。对应工单 [#28](https://github.com/T-miracle/Editor/issues/28)。

提交 `3f1ff579de2e96c495736c3c04660ed960a3ee89` 已推送 `origin/codex/markdown-plugin`，远端 SHA 一致。2026-10-04 读回 #28 为 closed / completed；父方案保持 open。

## 实现与边界

Markdown 解析留在独立 WASM 访客，采用 pulldown-cmark 0.13 的 CommonMark 与 GFM 表格、任务列表、删除线。插件将不可变内存文档转换为原生富文本、只读任务框、等宽代码和源码块范围；原始 HTML 显示为转义文字。图片本单使用替代说明，链接本单只读。

新增通用 `ui.richtext 1.0` 能力，保持 UI 文档版本 1。富文本、代码块和任意节点的源码范围均需协商能力；范围必须绑定源版本。所有输出受原有字节、深度、展开节点额度约束。原生 TextView 的默认图片访问与导航已覆盖，不能由标记绕过权限。`Environment.locale` 让插件提示跟随宿主语言。

复用已有编辑区预览贡献与原生可拖动分栏，无 Markdown ID、扩展名或语言名称宿主业务分支。EditorState 与 DocumentSession 保持唯一文档和撤销来源。

## 实际验证

在隔离的 `codex/markdown-plugin` 工作树运行，`CARGO_BUILD_JOBS=1`、`RUST_MIN_STACK=16777216`，使用工作树自己的 target，避免其他聊天的并发缓存污染。

- `cargo test -p editor-app delivered_markdown_preview_tracks -- --ignored --nocapture`：1 passed。实际发行 ZIP → Manager 安装 → 原生分栏与块布局；中文输入、一次粘贴、撤销／重做、未保存预览及磁盘不变；磁盘 reconciliation 重新加载且保持 clean；旧 revision 拒绝；关闭后重开产生不同身份，旧 worker 结果不能挂载控件；英文图片说明；深浅主题、切换文本文件、禁用和卸载撤销分栏。
- `cargo test -p editor-app role_ -- --nocapture`：2 passed。深浅主题及自定义富文本颜色，包括行内代码底色；代码块自定义角色改变实际原生行高。
- `cargo test -p editor-app markdown_language_package_restores -- --nocapture`：1 passed。前序语言资源仍在热安装、禁用和卸载时正确生效。
- `cargo test -p editor-app hover_card_tracks_symbol_after_window_resize -- --nocapture`：1 passed。共享文字样式未破坏已有悬浮提示原生行为。
- `cargo test --workspace --exclude editor-app`：通过；被标记 ignored 的真实 WASM 测试仍为跳过，不计入通过。
- `cargo check --workspace`：通过。

- `./scripts/build-plugins.ps1 -Packages markdown -HostExe target/debug/editor-app.exe`：独立构建并生成实际 `dist/plugins/markdown.zip`，上述原生测试使用该 ZIP。
- `./scripts/build-capability-example.ps1 -HostExe target/debug/editor-app.exe` 与 `cargo test -p plugin-runtime --test composable_ui -- --ignored --test-threads=1`：独立夹具 0.15.2 构建成功，完整组合测试 9 passed、0 ignored。新增 3 项以 `richtext-fixture` 身份验证 RichText／CodeBlock 协商、普通节点范围的能力门禁、中文字节偏移与非法范围拒绝；已有 6 项保持通过。
- `cargo test -p plugin-protocol richtext`：公开范围契约 1 passed。
- `cargo build -p editor-app` 与 `./scripts/verify-plugin-sdk.ps1 -HostExe target/debug/editor-app.exe`：通过 SDK 导出、损坏后修复和仓库外独立 WASM 构建，最终夹具版本 0.15.2。
- `cargo fmt --check`、`cargo fmt --manifest-path plugins/markdown/Cargo.toml --check`、`git diff --check`：通过。

## 审查与失败归因

规范审查指出 RichText 未继承角色颜色、行内代码使用 Base 浅色底色、代码块字体角色写死。均已修复，并补充颜色与实际布局回归。规格审查要求补独立能力夹具、重新加载、过期结果及英文场景，全部通过。最终两个独立审查轴均为 0 项。

首次红灯证实原资源包没有访客组件，不能提供预览。测试曾错误地将多行模拟键入视为一次撤销，改为单次原生粘贴验证原子编辑，键入另行验证；原有撤销实现未改变。关闭测试需使用实际规范化路径，原生控件协调发生在绘制时，测试已通过这些生产接缝观察结果。上游链接器 LNK4217 提示仍存在，不影响检查与测试结果。

三种模式、格式工具栏、实际图片、任务写回、导航、代码高亮、同步滚动及默认发行分别由后续工单交付，本单不宣称其完成。
