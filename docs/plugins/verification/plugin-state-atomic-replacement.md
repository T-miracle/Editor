# Windows 插件状态原子替换恢复

日期：2026-10-08。用户反馈每次修改后重新启动时，有概率出现 `failed to persist temporary file: 拒绝访问。(os error 5)`。

## 已确认的失败路径与限制

错误来自通用 `atomic_write` 的临时文件替换阶段。实际示例注册文件为普通 Archive 属性，目录和宿主 EXE 未读到 Low 完整性标签，当前工具进程为 Medium。该错误不能直接沿用之前的运行时独占锁或原仓库 Low 标签结论。

通过 Windows 读者保留 READ/WRITE 共享、暂时禁止 DELETE 共享，公共 `Manager::open_with_trust` 的真实启动保存产生了与截图完全相同的错误。回归 RED：0 passed、1 failed，0.06 s。实际外部占用进程尚未定位；本轮证明的是可复现的短暂占用失败模式，不能据此归因于防护软件或某个编辑工具。

## 修复与数据保护

- 保留同目录、同规范化父路径的原子替换及写入同步；不删除、截断原文件，不回退成非原子覆盖。
- 只对 Windows 替换阶段的错误 5/32/33 进行最多五次重试，退避为 10/20/40/80/160 ms。其他错误和其他平台立即返回；持续拒绝仍在重试结束后失败。
- 重试复用已经同步的临时文件，不重复写入候选内容。失败后释放候选文件，再返回包含具体目标路径的 IO 错误；不修改目标权限或只读属性。
- 所有插件状态、注册表、事务及收据共用该机制，没有插件身份分支，没有公开契约或插件包版本变更。

## 验证

| 反馈 | 实际结果 |
| --- | --- |
| 真实 Manager 启动、读者在 60 ms 后释放 | GREEN：1 passed、0 failed、0 ignored，0.08 s |
| 持续占用、原文件与临时文件清理 | 1 passed、0 failed、0 ignored，0.32 s；旧内容保留，错误含目标路径 |
| 只读目标 | workspace 测试通过；拒绝替换，保留只读属性与原内容 |
| 原生插件后台 actor 启动及公开给 UI 的状态 | 1 passed、0 failed、0 ignored，0.07 s；真实 Shutdown 应答证明 actor 成功启动，未发布致命管理器提示 |
| 实际示例注册信息副本 | 2 个已安装插件、20 次带短暂占用的启动全部通过，1.543 s；版本、digest、grants、启用选择保持一致，原始注册文件字节不变 |
| 实际 XML 包经公开管理器及原生交互 | 显式 `--ignored`：1 passed、0 failed、0 ignored，43.99 s |
| 共同检查 | `cargo fmt --check`、`cargo test --workspace --exclude editor-app`、`cargo check --workspace`、`git diff --check` 通过；保留原有警告 |

关键命令为 `cargo test -p plugin-runtime startup_registry_replacement_survives_transient_windows_reader -- --test-threads=1`、`cargo test -p editor-app --no-default-features plugin_manager_startup_recovers_from_transient_registry_reader -- --test-threads=1`。一次性副本探针位于忽略的 `target/xml-language-tools/plugin-persist-probe.rs`，只读取原始示例文件，在隔离目录调用真实管理器，不启动访客组件。

没有清空、重装用户插件，没有更改本机 ACL 或全局环境，没有提交、推送或关闭其他工单。若再次发生持续占用或权限问题，新增目标路径上下文用于进一步定位，而不是承诺外部占用永远不会发生。

## 实际 release 重新启动

优化构建通过（1 min 53 s），产物为 `target/native/release/plugin-persist-preview/editor-app.exe`。原有示例配置和插件目录直接复用，原生窗口 PID 23764 创建成功。首次启动检查确认管理器持有原目录的运行时租约，`registry.json` 的写入时间晚于本次进程启动时间，stderr 为空。后续检查时该窗口已退出，未将此启动检查写成持续存活或可见 UI 无报错的证明。
