# 工单 04：声明式配置验证

对应 [GitHub #5](https://github.com/T-miracle/Editor/issues/5)，规格 T23。审查固定点为 `c2ca98dcedd83eaf4acae0e542b5403e108536b0`，包含工作区差异和新增文件。

## 交付行为

`configuration` 1.0 支持布尔、限定长度文本、范围整数、枚举及有效值来源。设置窗口新增插件设置页，使用本地原生控件；用户点击应用或重置后，通过既有 worker 交给 Manager，保存成功才发布新值。项目值只来自用户确认的宿主本机记录，不自动信任项目仓库文件。

优先级为已确认项目值、用户值、发现值、默认值。每个显式层级先校验，较高优先级不能掩盖无效值。可选 WASM Validate 钩子只能补全默认来源的项；错误保留旧配置和实例。Apply 将最终有效配置送入候选实例，再激活。

本版生效方式为 `restart_instance`：全局修改准备所有受影响的活动及暂存工作区实例，项目修改只影响所属实例。全部准备、校验和激活成功后持久化，再替换旧实例并撤销旧资源。私有数据提交失败执行既有回滚；不宣称外部进程副作用可回滚。禁用插件仅运行受限准备与校验，不能因改设置而自动激活。

配置位于宿主管理目录，不能通过插件私有存储接口读取或改写。只允许声明的键；项目作用域不能覆盖用户专属键，也不能修改信任、权限或宿主偏好。普通卸载保留配置，明确删除数据同时删除配置。示例更新为 0.4.0，继续由公开 SDK 独立构建。

## 验证记录

| 验证 | 结果 |
| --- | --- |
| `cargo fmt --check`、独立示例格式检查 | 通过 |
| `cargo check --workspace` | 通过，2 条既有死代码警告 |
| `cargo build -p editor-app`、`scripts/build-capability-example.ps1` | 通过，公开 SDK 构建和 README 打包正常，保留既有链接警告 |
| `cargo test --workspace --exclude editor-app` | 37 项通过，12 项真实组件测试默认忽略 |
| `cargo test -p editor-app --bin editor-app -- --test-threads=1` | 156 项通过，8 项默认忽略 |
| `cargo test -p plugin-runtime --test settings -- --include-ignored` | 3 项通过：声明、覆盖、发现、错误、持久化、跨工作区、禁用和卸载数据策略 |
| `cargo test -p editor-app --bin editor-app native_plugin_settings -- --ignored --test-threads=1` | 通过：实际设置弹窗入口、原生表单确认、真实 Manager/WASM 应用、来源显示及钩子错误反馈 |
| 前序 `capability_packages`、`scoped_instances`、`editor_requests` 的真实 WASM 回归 | 10 项通过 |
| GPUI `typed_editor_requests` 和 `capability_package_consent` 显式运行 | 通过 |
| `cargo run -p plugin-runtime --example ui_smoke -- dist/plugins/example.zip` | 通过 |

日志位于忽略目录 `target/plugin-api-publication/settings-*.log`。保留编译与行为 RED/GREEN，包括默认来源、优先级、发现值及删除数据后配置残留。GPUI 测试通过既有 worker 发布边界执行真实运行时操作，不是人工桌面验收；沿用串行编辑器测试策略。

## Standards

独立规范审查及最终复核均为 0 项发现。代码包含意图注释，原生表单复用本地控件，配置命名空间与授权边界独立。

## Spec

独立规格审查及最终复核均为 0 项发现。已覆盖 T23 的有效配置来源、可选钩子、热应用、作用域限制及失败保持旧值。审查者只读审查，测试由主代理执行。
