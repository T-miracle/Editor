# 组合 UI 验证记录

状态：已完成（2026-10-03 状态汇总）。本记录保留该阶段实际执行的结果与限制；后续迁移、旧协议删除及最终交付以[完整契约验收](plugin-api-contract-verification.md)为准。20 张工单均已提交、推送并关闭；早期正文中的“尚未推送”“后续工单”是当时的历史状态。

对应工单：[#11](https://github.com/T-miracle/Editor/issues/11)，规格 T15、T22。基线 `5273c784e56a69f7126968c3f598750fb2bf2fa0`。

## 交付

- 原生布局树增加 Canvas 节点，标准表单、纯画布和混合界面走同一声明路径；分别协商 `ui.native`、`ui.canvas`、`ui.grid`，不包含插件 ID 判断。
- 每个画布独立处理焦点、UTF-16 组合输入、键盘、左中右鼠标、滚轮与尺寸；稳定节点在重绘和主题变化时保留本地状态。背景模态节点、失效版本和错误事件被拒绝。
- 通用 SVG 后台渲染器同时服务旧场景和新组合树，保持颜色、透明度及资源读取限制；整份文档统一限制节点、绘图数量和编码尺寸。
- 编辑区预览申请文档读取权限，接收带 DocumentVersion 的未保存文本并原样回传 source；宿主丢弃过期预览。隐藏、失活和卸载撤销布局及原生事件目标。
- 独立 capability-example 0.8.0 通过公开宿主 SDK 构建，ZIP 携带布局资源与 README；工具栏缩放、输入框、三种布局与预览逻辑全部在 guest 内。

## 验证

实际执行命令（额外 SDK 包测试需先构建宿主并运行打包脚本）：

```powershell
cargo fmt --check
cargo fmt --manifest-path plugins/capability-example/Cargo.toml --check
cargo check --workspace
cargo test --workspace --exclude editor-app
cargo test -p editor-app -- --test-threads=1
cargo build -p editor-app
./scripts/build-capability-example.ps1
cargo test -p plugin-runtime --test composable_ui -- --ignored --test-threads=1
cargo test -p editor-app composed_wasm_preview -- --ignored --test-threads=1
```

编辑器普通测试 167 项通过，15 项需要外部夹具的测试保持显式执行；本次新增独立 WASM→GPUI 测试已单独通过。组合包测试覆盖无文档/进程权限组合、能力缺失与可选网格、失效事件、文档授权和版本、真实工具栏缩放及提供三种布局。

GPUI 通过现有 worker 发布与输入入口覆盖中文、空格、IME 组合文字和 emoji、主题字号更新、禁用后重新启用的首次尺寸，以及跨边界鼠标释放。真实独立 WASM 测试打开编辑器文档、渲染 SVG、点击放大、输入未保存文字并验证磁盘未变，最后卸载确认编辑区分栏与画布均回收。

回归先复现再修复了 Unicode 字节数误判、禁用画布漏发尺寸、外部释放丢失以及非焦点画布重绘清除拖动归属。规范轴和规格轴复审均通过。Windows 链接器已有 wasmtime/tree-sitter 导入警告及两处既有 dead_code 警告仍存在，未阻止编译或测试。本次不声称已完成所有人工桌面验收；完整迁移后的统一验收仍由 #21 承接。
