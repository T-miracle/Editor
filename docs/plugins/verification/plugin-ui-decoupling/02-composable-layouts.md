# 工单 02：可组合布局与提供者选择验收

日期：2026-10-06。议题：[#63](https://github.com/T-miracle/Editor/issues/63)。
分支：`codex/plugin-ui-decoupling`；固定审查起点：`4d4a7b6`。
状态：实现、阶段检查及针对性验证通过，双轴审查进行中；议题未关闭。

## 已实现的公开边界

`editor.layout 1.0` 用版本化文件树表达整体中心布局；唯一 `NativeEditor` 引用现有文本会话。
校验拒绝无上下文、重复、跨文件及弹窗／工具栏编辑器引用。辅助贡献不具有整体布局权。
宿主只借用原生输入、绘制和会话资源；布局节点来自真实独立包。

文件标签右键菜单选择兼容提供者并提供文本恢复，按工作区和文件类型记忆。
多候选不按安装顺序或 ID 自动选中；唯一可用候选首次采用后记忆。
新安装不覆盖选择，失效保留用户意图，文本回到原编辑器，非文本保留 Tab 和替代查看器菜单。
选择推进目标版本，迟到源／UI 事件不能改变当前目标。

## 当前行为证据

- 公共 JSON 接缝先因未知 `native_editor` 失败，新增契约后通过；协议 30 项测试通过。
- `cargo check --workspace`：开发中的针对性编译通过。
- 粗工单边界 `cargo fmt --check`、`cargo test --workspace --exclude editor-app`、`cargo check --workspace`：通过，ignored 不计入默认测试覆盖。
- `cargo test -p editor-app preview_tests -- --test-threads=1`：既有 SVG 原生源码／分割行为通过。
- `cargo build -p editor-app` 与 `./scripts/build-layout-example.ps1`：独立 SDK 构建实际 `layout-example.zip`，不引用仓库内部 crate。
- `cargo test -p editor-app real_layout_packages -- --ignored --test-threads=1`：实际不同身份包验证并排、上下、仅插件内容、未保存中文输入、选区、Undo／Redo、显式切换、后装不覆盖、失效回退、重启恢复和工作区隔离。
- 同一原生路径检查真实中文预编辑组合态跨布局保留、隐藏输入不接收提交、恢复复用当前状态；PNG 失效后的替代查看器及不出现文本恢复菜单通过。
- `cargo test -p plugin-runtime --test composable_layouts -- --ignored`：真实包的协商、辅助布局拒绝、精确引用、迟到源与 UI 版本拒绝及撤销通过。
- 当前分支宿主运行 `./scripts/verify-plugin-sdk.ps1`：导出、完整缓存修复及仓库外独立组件构建通过，新增布局契约测试随 SDK 分发。
- `cargo test -p plugin-runtime --test sdk_distribution -- --ignored`：当前导出 SDK 的独立组件从公开管理器安装通过，1 项通过。
- 最新独立夹具重新构建后，`real_layout_packages` 原生用例 1 项通过，`image_package_draws` 实际 Image 包回归 1 项通过；SVG 源码与 PNG／JPEG／GIF／WebP 预览保持有效。

初次独立夹具遗漏 Snapshot，公开 Manager 安装明确拒绝，补齐状态契约后通过。
持久化测试初次传入非规范化临时路径；改为实际工作区规范化路径后通过，产品仍沿 Workspace 根路径保存。
这些记录不代表最终跨插件 U01–U20 全套验收，实际 Markdown／Image／Terminal 的模式与工具迁移由 04 接续。
