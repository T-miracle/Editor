# 03 — 原生视图模式验收

> 历史验收记录：以下结论仅对应 Markdown 0.3.0 和当时提交。2026-10-06 的插件 UI 解耦已移除 `editor.presentation`、`view_modes` 及 `editor_presentation` 测试；当前模式、图标与共享偏好由插件通过 `editor.layout`、`ui.tools` 和 `storage.private` 提供。现行证据见 [插件迁移验收](../../../../docs/plugins/verification/plugin-ui-decoupling/04-plugin-migration.md) 与 [最终契约验收](../../../../docs/plugins/verification/plugin-ui-decoupling/05-integration-and-contract.md)，不要把下面的旧命令作为当前构建入口。

日期：2026-10-04。基线：`3f1ff579de2e96c495736c3c04660ed960a3ee89`。Markdown 包版本：`0.3.0`。对应工单 [#29](https://github.com/T-miracle/Editor/issues/29)。状态：原生模式、公开契约、仓库必需检查及 Standards / Spec 双轴审查通过。

提交 `4f2c9bd1411c9b64c4058a74d1db016e39e15387` 已推送 `origin/codex/markdown-plugin`，远端 SHA 一致。2026-10-04 读回 #29 为 closed / completed。

## 实现与边界

新增可选公开 `editor.presentation 1.0` 能力和编辑区面板的 `view_modes` 声明。包提供三个几何 SVG，宿主通用控件读取已授权实例的图标，在底栏现有工具组右侧添加短竖线和仅编辑、分栏、仅预览按钮。图标使用 16 × 16 设计网格，按本地按钮的 14 px 尺寸绘制，具备中英文提示和项目主题选中态。

布局只组合现有源编辑器和原生预览，不替换 EditorState、选区、DocumentSession 或撤销栈。模式按工作区及贡献身份记忆；切文档、重开保持偏好，另一工作区独立。仅预览实际绘制时回收隐藏源码的焦点，避免文档激活或异步启动恢复延迟抢回焦点。禁用贡献恢复普通编辑区。

图标声明要求必需能力、工作区实例及编辑区位置；公开图标读取和包校验限制文件大小、几何 SVG 内容及规范化安装版本路径，拒绝外部引用和符号链接逃逸。无 Markdown 专属宿主分支。同步开关归工单 10；源码工具栏归工单 04，并接在当前源码容器内。

## 实际验证

在隔离工作树 `codex/markdown-plugin` 中使用独立 target，设置 `CARGO_BUILD_JOBS=1`、`RUST_MIN_STACK=16777216`。

- `./scripts/build-plugins.ps1 -Packages markdown -HostExe target/debug/editor-app.exe`：构建真实 `0.3.0` ZIP，包含访客组件和三个图标。
- `cargo test -p editor-app delivered_markdown_modes_remember -- --ignored --nocapture`：1 passed、0 failed、0 ignored。实际 ZIP 经 Manager 安装后验证默认分栏、底栏位置、鼠标和原生 Tab／Space、切文档模式、选区及实体身份保留、一次撤销、仅预览占满正文、同工作区新窗口恢复、另一工作区独立、浅色主题及禁用撤销。
- `cargo test -p plugin-runtime --test editor_presentation -- --ignored --test-threads=1`：4 passed、0 ignored。不同插件身份的独立真实 WASM 包覆盖协商、缺失或可选能力拒绝、非编辑区或非工作区贡献拒绝、外部引用／畸形／超限图标拒绝、安装后篡改及真实 Windows junction 越界。
- `cargo test -p plugin-runtime --lib installed_panel_selects_light_and_dark_icons`：1 passed，保留原面板图标行为。
- `cargo test -p editor-app svg_preview_follows_open_documents -- --nocapture`：1 passed，原 SVG 预览仍跟随打开文档和未保存编辑。
- `cargo test -p editor-app delivered_markdown_preview_tracks -- --ignored --nocapture`：1 passed、0 ignored，前序 Markdown 实际包的原生编辑、过期结果和生命周期仍通过。
- `cargo fmt --check`、`cargo test --workspace --exclude editor-app`、`cargo check --workspace`：通过；workspace 中其他 ignored 用例仍为跳过，不计为通过。
- `cargo build -p editor-app`、`./scripts/verify-plugin-sdk.ps1 -HostExe target/debug/editor-app.exe`：通过公开 SDK 导出、损坏修复与仓库外独立组件构建，含本单公开类型和专题文档。
- `git diff --check`：通过。

## 审查与失败归因

初始原生红灯确认 0.2.0 包没有模式按钮。键盘测试最初使用只发 key-down 的模拟文本工具，不能触发 Base Button 的 key-up 激活；现改为上游测试采用的原生完整事件对，并保留 Tab 后切换分栏的行为断言。追加回归真实复现仅预览切文档后隐藏源码抢焦点，通用绘制层修复后切文档与重开两种场景通过。

Standards 审查要求区分图标设计网格和实际显示尺寸，README 已修正。最终 Standards 与 Spec 均为 0 项，必需检查通过。既有 Wasmtime 链接器 LNK4217 提示未改变，不视为本单新增失败。
