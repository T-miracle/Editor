# 工单 01：新能力协议首条路径验证

日期：2026-10-02。对应 [GitHub #2](https://github.com/T-miracle/Editor/issues/2)。实现已提交为 `19690946316b3557058e10e240f650d5170e86b4`，GitHub #2 已按用户授权关闭并读回确认。提交尚未推送，未发布正式插件包。

## 交付范围

基础 API 与各能力独立协商版本；当前接入 `package.assets` 和 `ui.native`。必需能力不匹配在激活前拒绝，可选能力缺失由插件读取协商结果后降级。资源读取检查声明权限、协商能力、路径及配额。SDK 封装请求 ID、类型化结果和错误；宿主不按示例插件 ID 分支。

独立 `capability-example` 经宿主公开 `--plugin-cargo` 编译为真实 WASM 组件，包中包含 README 和文本资源。安装确认、权限检查、激活、原生文本呈现和卸载沿现有 Package/Manager、GPUI 边界验证。产物只在 `target/plugin-api-test/capability-example.zip`，不加入正式发行目录。

新形式使用传输标记 `protocol = 7`，清单内基础 API/能力版本与包版本分离。开发过渡期间保留旧形式；不宣称已完成全平台迁移。异步请求、工作区实例、语言/LSP、事务更新及旧接口删除依照后续工单实施。

## 验证结果

| 验证 | 结果 |
| --- | --- |
| `cargo fmt --check` | 通过 |
| `cargo check --workspace` | 通过，保留 2 条现有死代码警告 |
| `cargo test --workspace --exclude editor-app` | 36 项通过；2 项真实 WASM 用例需独立构建后显式执行 |
| `cargo test -p editor-app --bin editor-app -- --test-threads=1` | 155 项通过，6 项默认忽略 |
| `scripts/build-capability-example.ps1` | 公开 SDK 构建、WASM 与资源打包通过 |
| `cargo test -p plugin-runtime --test capability_packages -- --ignored` | 2 项通过：安装/降级呈现/类型化错误/卸载，以及权限与能力负向验证 |
| `cargo test -p editor-app --bin editor-app capability_package_consent -- --ignored` | 原生安装确认、文本渲染与卸载验证通过 |
| `cargo run -p plugin-runtime --example ui_smoke -- dist/plugins/example.zip` | 旧协议真实 WASM 界面、事件、弹窗与更新恢复回归通过 |

真实 WASM 测试不是靠忽略标记免验收：上表中显式执行的用例使用本次构建产物。GPUI 测试沿已有工作线程发布边界注入 Manager 的真实场景，不是人工桌面操作验证。

默认并行的编辑器全套测试曾出现 1 项失败：`app::plugins::tests::project_only_rust_startup_survives_registry_refresh` 在读取空列表时越界。单独重跑通过，全套串行两次通过。尚未证明该并发不稳定的根因，不能将默认并行套件描述为稳定通过；本任务没有修改该既有测试。

回归过程记录保存在忽略目录 `target/plugin-api-publication/`：`quota-red.log` 记录 SDK 将超限错误误判为 ID 不匹配的失败；`quota-green.log` 验证修复。最终检查分别为 `final-workspace-tests.log`、`final-check.log`、`final-editor-tests.log` 与 `final-ui-test.log`。

## Standards

规范审查发现 1 项 P2：超大请求在宿主安全解码前被拒绝，返回 ID 为零，SDK 将配额错误覆盖为响应关联失败。已通过共享请求大小常量、SDK 路径长度预检查与编码后检查修复，并增加真实 WASM 回归。复核通过，无遗留发现。

## Spec

规格审查未发现工单 01 缺项或未经要求的范围扩张。后续工单能力不作为本工单验收要求。

审查汇总：Standards 1 项已修复、0 项遗留；Spec 0 项发现。

## 提交核验

用户另行授权提交所有工作区代码并关闭对应工单。此前积累的编辑器、插件与工作区交互改动提交为 `6d2b0fd`，悬浮提示修复提交为 `e60b353`，本工单单独提交为 `1969094`。拆分时通过内容快照核对，未覆盖原工作区代码。

前置提交在隔离文件树完成格式检查、工作区测试和编译检查；编辑器 154 项、终端 43 项、SVG 5 项测试通过。悬浮提示提交另通过 10 项相关测试及工作区检查。新能力协议提交与前述完整验证时的代码逐文件一致，复用本页记录的验证结果。

宿主侧边栏协议及插件 ID 迁移与终端调用方必须同步提交：尝试分离时，隔离编辑器测试出现 4 项失败，合并配套变更后全部通过。独立插件编译使用仓库外的临时目录，避免嵌套 Cargo 工作区影响验证。

GitHub 连接器缺少写入权限，关闭操作改用仓库已配置的 Git 认证完成；凭据未写入文件或日志。仅关闭 #2，设计基线及后续工单保持开放。
