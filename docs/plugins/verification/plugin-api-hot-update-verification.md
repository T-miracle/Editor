# 插件热更新验证（GitHub #15）

状态：已完成（2026-10-03 状态汇总）。本记录保留该阶段实际执行的结果与限制；后续迁移、旧协议删除及最终交付以[完整契约验收](plugin-api-contract-verification.md)为准。20 张工单均已提交、推送并关闭；早期正文中的“尚未推送”“后续工单”是当时的历史状态。

## 交付边界

沿用 `Manager` 的数据事务。宿主线程捕获安装任务，后台完成解包、WASM 编译、隔离副本的预览迁移和依赖准备；原工作线程继续执行旧实例命令、编辑器请求和文档事件。配置、启停和卸载操作在准备结束后串行处理。

正式切换先封闭旧请求与语言服务，再获取最终快照、停止旧实例、重新复制最新私有数据并执行迁移。最终依赖计划必须与已准备计划相同。候选激活成功才提交注册表与数据；切换失败通过旧快照恢复旧包，但分配新的实例身份。更新与恢复同时失败时保留两个错误。

界面事件携带生成时的实例代际；组件输入回调、滚动条及延迟更新不跨代投递。语言服务发布新的资源所有者，让已打开文档重新发送当前内存文本。数据事务不回滚用户文档、原生进程 PID 或已经发生的外部副作用。

## 已约定的公共验证接缝

- 独立构建的 SDK WASM 包，经真实 `Package` / `Manager` 接口运行；夹具组合私有文件、原生 UI、版本化插件服务和真实 stdio LSP。
- 生产 `Worker` 的消息与发布通道；本地 HTTP 门闩暂停依赖下载，同时验证旧命令和真实编辑器请求仍能完成。
- GPUI 编辑器和语言服务发布接缝；真实输入形成未保存文本，由正常诊断调度触发 `didOpen`，验证更新和回退后自动重新同步。
- 原有私有数据迁移、崩溃恢复及实例作用域回归继续适用。

## 执行命令

```powershell
cargo fmt --check
cargo check --workspace
cargo test --workspace --exclude editor-app
cargo test -p plugin-runtime --test hot_update -- --ignored --nocapture
cargo test -p plugin-runtime --test data_migration -- --ignored --test-threads=1
cargo test -p editor-app -- --test-threads=1
cargo test -p editor-app extensions::worker::worker_tests -- --ignored --nocapture --test-threads=1
cargo test -p editor-app hot_update_and_rollback_resync_unsaved_open_document -- --ignored --nocapture --test-threads=1
```

忽略的验收测试需先通过公共 SDK 构建 `capability-example.zip`，并构建 `plugin-runtime` 的 `lsp_fixture` 示例。全部使用隔离临时目录，不读取或修改用户真实插件安装数据。

2026-10-03 验证：格式检查、工作区检查及非 UI 工作区测试通过；编辑器常规测试 170 项通过、21 项按前置条件忽略。独立执行的组合更新回归 1 项、生产 Worker 回归 2 项、未保存文档 GPUI 回归 1 项及数据迁移/崩溃恢复回归 5 项全部通过。组合回归额外注入旧包资源损坏，确认更新失败和恢复失败同时保留、失败所有者不再运行。规范与规格两轴审查通过。
