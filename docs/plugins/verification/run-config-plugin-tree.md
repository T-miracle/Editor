# 运行配置插件模板与配置树：分阶段验收

规格：[run-config-plugin-tree-v1](../specs/run-config-plugin-tree.md)。分支：`Editor-run-debug-build`；主工作区未合并。

## 01 / GitHub #68

本阶段交付公开 `run.configurations 1.0`、有界异步调用、独立插件模板与原生表单、本机存储、应用／保存／取消及实际执行的最小闭环。沿用此前已确认的原生父子弹窗、选择器和本地按钮外观，必要的未提交前置实现随本切片纳入。

两个独立包 `configuration-alpha`、`configuration-beta` 由公共 SDK 在仓库外构建，再通过公开管理器安装；它们分别提供纵向、横向原生布局。夹具不是宿主白名单，真实 Rust 与终端插件由 04 接入。

| 场景 | 可观察证据 |
| --- | --- |
| C01、C02 | 原生 owned dialog、空选择提示、重开跟随外部配置，不自动选择另一条 |
| C04、C05、C08 | 添加抽屉尺寸与右栏位置；分组模板、默认值、同模板独立身份，添加不写盘／执行 |
| C06、C07 | 两包经公开服务编辑；保存重开恢复；真实进程收到含空格、引号、中文、元字符的原参数数组 |
| C27 | 规范化工作区键的本机写盘与重读；项目未产生共享配置文件 |
| 最低失败保护 | Apply 后 Cancel 保留已应用数据；无效保存禁止执行；写盘失败保留原生窗口与草稿 |

原生批次：`cargo test -p editor-app plugin_configuration -- --ignored`。前置：`cargo build -p editor-app`、`./scripts/build-configuration-example.ps1` 和已有实际终端包。SDK 分发：`./scripts/verify-plugin-sdk.ps1`；读者文档：在 `website/` 执行 `node --test tests/doc-sources.test.mjs`。

实现过程中发现并修复：双栏 flex 的垂直居中导致内容滚动区零高度；用空串模拟键入未删除原文本的测试误判；夹具原始路径与应用规范化存储键不一致；测试泵把尚未完成的公开执行查询当作未知状态。通过物理点击、键入、实际包和真实进程验证修复；未开放私有 Reference 或新增测试专属宿主 API。

2026-10-07 最终提交候选通过检查。候选通过 `git checkout-index` 导出到隔离目录 `target/run-config-plugin-tree/candidate-01`；实际 ZIP 夹具来自同一已纳入候选的源码与 SDK。检查日志保存在本机 `target/run-config-plugin-tree/`，不提交生成产物：

- `cargo fmt --check`、`cargo test --workspace --exclude editor-app`、`cargo check --workspace`：退出 0；非 UI 测试中的 ignored 项不计为通过。
- `cargo test -p editor-app -- configuration_icons selector sdk_export`：9 passed，0 failed。
- `cargo test -p editor-app plugin_configuration -- --ignored`：3 passed，0 failed，未跳过这三项实际包验收。
- `cargo build -p editor-app`、`./scripts/build-configuration-example.ps1`、`./scripts/verify-plugin-sdk.ps1`：退出 0，独立构建与 SDK 导出／修复通过。
- `website/` 的 `node --test tests/doc-sources.test.mjs`：9 passed，0 failed。
- `git diff --cached --check`：通过。

02–04 的目录树、完整事务、故障边界及实际插件交付仍待实施。原生输入法候选窗口没有在本阶段被宣称通过。

## 02 / GitHub #69

待实施：C03、C11–C19、C22。

## 03 / GitHub #70

待实施：C20、C21、C23–C26。

## 04 / GitHub #71

待实施：C09、C10、C28–C30，最终场景矩阵和实际插件包验收。
