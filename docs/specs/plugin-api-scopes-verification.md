# 工单 02：工作区实例与资源归属验证

对应 [GitHub #3](https://github.com/T-miracle/Editor/issues/3)，规格 T16、T17、T22。审查基线为 `d7eec61f8c0c71e4727da7dbc8c7c308917e3d18`；比较该提交后的工作区变更及新文件，不包含前序工单。

## 交付行为

协议 7 清单新增 `scope`，缺省 `workspace`，显式 `application` 的实例独立于工作区存活，且不继承当前工作区根。一个应用拥有一个 Manager；其逻辑工作区共享安装包与编译引擎，各自保存 WASM 实例、界面、句柄、私有文件与快照。当前只增加逻辑边界，不新增多窗口产品功能。

`workspace.files` 1.0 提供所属工作区只读访问；`storage.private` 1.0 提供私有文件读写。SDK 封装打开、读取、写入与释放；请求同时检查能力协商、权限、实例状态、作用域与路径。句柄携带随机实例 UUID、作用域与资源槽；释放、关闭或替换后旧句柄不复活。

私有文件放在 `data/<id>/workspaces/<workspace-hash>/files` 或 `data/<id>/application/files`；宿主快照为同级 `state.json`，用户设置仍保存在用户配置目录。工作区读取不能写入，私有写入为根目录直接子文件、原子替换，受声明总配额、单文件 1 MiB 和请求编码 2 MiB 限制。声明式包仍不创建 WASM 或编译引擎。

项目停用只回收当前拥有者；全局停用、卸载与宿主结束统一回收相关拥有者。普通工作区关闭保存最后快照，撤销信任则直接停止 guest，保留上次快照，避免撤销后再进入具有旧权限的 Snapshot 回调。更新失败回滚会分配新拥有者，并提交恢复期间的暂存写入。

设置新增“信任此工作区并允许插件运行”，保存在宿主本地会话文件，项目文件不能授予信任。受限启动不加载插件/声明式贡献；撤销时立即屏蔽排队的场景、效果和贡献，再由原有编辑器同步路径移除语言服务与面板。保留编辑器既有的默认信任行为，本工单未引入首次打开工作区的确认产品流程。

## 范围边界

- 当前仍是扩展—迁移—收缩阶段，旧协议私有数据格式保持原样；既有插件迁移及旧协议删除依照后续工单执行。
- 其他逻辑工作区仍有同包实例时，替换包会明确拒绝；跨作用域热更新事务属于工单 14（GitHub #15）。改变清单实例作用域要求重新安装，不能隐式搬迁数据。
- 用户配置能力、进程/订阅新协议和故障自动恢复按后续工单交付；这里提供后续资源归属的基础。
- GPUI 验证使用已有 worker 发布边界和真实 WASM 产物，不是人工桌面操作。

## 验证记录

| 验证 | 结果 |
| --- | --- |
| `cargo fmt --check` | 通过 |
| `cargo check --workspace` | 通过，保留 2 条既有死代码警告 |
| `cargo test --workspace --exclude editor-app` | 36 项通过；7 项真实组件测试默认忽略，已另行显式运行 |
| `cargo test -p editor-app --bin editor-app -- --test-threads=1` | 156 项通过，6 项默认忽略 |
| `scripts/build-capability-example.ps1` | 宿主公开 SDK 构建、WASM 与 README 打包通过，示例升至 0.2.0 |
| `cargo test -p plugin-runtime --test scoped_instances -- --ignored --test-threads=1` | 5 项通过；后续扩展的项目路径别名更新回归另行通过 |
| `cargo test -p plugin-runtime --test capability_packages -- --ignored --test-threads=1` | 2 项通过 |
| `cargo test -p editor-app capability_package_consent -- --ignored --test-threads=1` | 真实组件的安装确认、原生文本、信任撤销与卸载回收通过 |
| `cargo run -p plugin-runtime --example ui_smoke -- dist/plugins/example.zip` | 旧协议界面、事件、弹窗与更新恢复通过 |

忽略目录 `target/plugin-api-publication` 保存实际日志：作用域与句柄的编译 RED、项目停用行为 RED、信任 UI RED、路径别名更新 RED、启动阶段撤销信任 RED，以及对应 GREEN。应用级生命周期与回滚持久化为真实组件回归，不宣称这些用例均观察过独立 RED。最终检查见 `scope-fmt.log`、`scope-workspace-tests.log`、`scope-check.log`、`scope-editor-tests.log`、`scopes-final.log`、`scope-alias-green.log`、`scope-ui-final.log`。

沿用前序验证的串行编辑器测试方式；前序默认并行测试曾出现贡献目录相关失败，根因未确认，本工单没有据此宣称默认并行测试稳定。真实组件套件执行中曾同时启动同一测试目标编译，Windows 拒绝覆盖正在运行的 EXE；等待原测试结束后顺序重跑通过，此次链接错误不是行为测试 RED。

## Standards

初审发现回滚写入仅保存在 staging、项目停用波及其他工作区；复核发现更新恢复的项目覆盖漏用 canonical 身份比较。分别修复提交 staging、分离项目/全局停用、统一路径身份，并补真实 WASM 回归。独立审查者最终复核通过：3 项已修复，0 项遗留。

## Spec

初审发现撤销信任后 Snapshot 仍能使用旧权限、项目停用跨工作区影响；复核发现路径别名下更新可能丢失项目覆盖。撤销改为不调用 guest，其他问题与 Standards 同步修复。独立审查者最终复核通过：3 项已修复，0 项遗留。

主代理另补启动时贡献目录已加载而 worker 尚未发布 entries 的竞态：撤销信任显式清空贡献目录，新增 GPUI 回归验证不再保留语言声明。两个审查轴均复核此改动。
