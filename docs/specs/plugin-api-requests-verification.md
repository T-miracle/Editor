# 工单 03：类型化请求与文档事件验证

对应 [GitHub #4](https://github.com/T-miracle/Editor/issues/4)，规格 T12、T13、T22。审查固定点为 `bc3102ff0f70803726b3c70f0746d76d4f9c807c`；比较该提交后的工作区差异和新增文件。

## 交付行为

新增独立协商的 `editor.documents` 1.0 与 `ui.panels` 1.0。工作区插件通过类型化操作读取选区、读取活动目录、保存精确版本的打开文档、显示或隐藏自身声明面板。分别检查 `editor.read`、`editor.write`、`ui.panels`，不会通过命令字符串或插件 ID 特例执行。应用级实例不获得当前工作区编辑器权限。

WASM 请求在运行线程受理，返回实例所有的请求句柄；既有 worker 发布边界把请求送到 GPUI 编辑器线程，完成或失败通过类型化通知返回。`EditorTask` 在 SDK 内关联通知，调用方只需保留当前意图的任务。示例 0.3.0 提供对应菜单命令，受理、进度、成功和失败均显示在原生面板。新意图即使被拒绝也解除旧任务关联，旧成功不能覆盖新错误。

每实例最多 32 个请求，worker 发布队列和编辑器待执行队列各最多 256 个；满额明确失败。请求截止时间为 1–300000 毫秒；终态不可覆盖。取消在不可逆操作前阻止执行，返回 `NotExecuted`；原子写入或显隐操作进入提交后只能停止等待，返回 `WaitingStopped`。`TryTerminate` 是尽力而为，当前原子操作不能保证终止，不能将取消解释为回滚。实例停用、卸载、释放请求或撤销信任都会结清仍未完成的任务。

保存采用编辑器文本的不可变快照。后台检查原磁盘摘要、记录历史并写入同步后的临时文件；编辑器线程再次核对信任、打开实体、revision、路径和磁盘内容后，经 `DocumentSession` 原子提交。取消或校验失败不会写目标文件，临时文件随对象释放；准备期间生成的本地历史快照不回滚。用户保存与同路径插件保存串行，编辑、重命名或关闭使旧请求失效。外部进程不遵循宿主锁，因此保留既有文件保存的最终检查与原子替换之间的系统级竞争窗口，不宣称跨进程事务锁定。

文档身份属于打开的编辑器实体，路径为工作区相对路径。用户编辑、静默磁盘重载和重命名均推进版本；磁盘文件暂时删除不会误报标签页关闭。真正关闭发布终止版本。事件是版本提示，不是文本增量日志，不承诺重放订阅前的历史。

每实例最多 8 个可释放订阅，每队列最多 64 个待处理文档，合并同一实体的最新版本并按 FIFO 公平分批投递。最多保留 1024 个实体的跨批次版本水位；预算耗尽或源队列溢出以 `SubscriptionFailed/LimitExceeded` 明确结束。示例清除失败句柄，允许一次点击重新订阅。回调内释放其他订阅后，同批次剩余通知（包括失败通知）不再投递。请求最终结果单独保留至投递或实例退出，不按可合并进度静默丢弃。

## 验证记录

| 验证 | 结果 |
| --- | --- |
| `cargo fmt --check` 及独立示例 `cargo fmt --manifest-path plugins/capability-example/Cargo.toml --check` | 通过 |
| `cargo check --workspace` | 通过，2 条既有死代码警告 |
| `cargo build -p editor-app` | 通过，保留既有 Windows 链接警告 |
| `cargo test --workspace --exclude editor-app` | 36 项通过；10 项真实组件测试默认忽略，已另行显式运行 |
| `cargo test -p editor-app --bin editor-app -- --test-threads=1` | 156 项通过，7 项默认忽略 |
| `scripts/build-capability-example.ps1` | 独立公开 SDK 构建 WASM、README 打包通过，版本 0.3.0 |
| `cargo test -p plugin-runtime --test editor_requests -- --ignored` | 3 项通过：受理/进度/完成、关联/拒绝/配额、取消/超时/销毁、版本合并/跨批次旧事件/释放/溢出/重新订阅 |
| `cargo test -p editor-app --bin editor-app typed_editor_requests -- --ignored --test-threads=1` | 通过：真实 worker 发布到 GPUI、选区、保存、面板显隐及宽度回收、静默重载、外部冲突、重命名、磁盘删除保留打开身份、文档事件发布 |
| `cargo test -p plugin-runtime --test scoped_instances --test capability_packages -- --ignored --test-threads=1` | 前序 7 项真实组件契约回归通过 |
| `cargo test -p editor-app --bin editor-app capability_package_consent -- --ignored --test-threads=1` | 真实组件安装确认、原生显示、信任撤销与卸载回收通过 |
| `cargo run -p plugin-runtime --example ui_smoke -- dist/plugins/example.zip` | 旧协议界面、事件、弹窗、兼容拒绝及更新恢复通过 |

GPUI 验证使用既有发布边界和真实 WASM 产物，不是人工桌面操作。运行时保留请求不执行、显式推进截止时间的可控夹具验证慢任务，不使用实际长时间睡眠。保存测试控制编辑器调度顺序插入外部改写和重命名，不宣称覆盖每一个操作系统时序。

忽略目录 `target/plugin-api-publication` 保存日志：请求与订阅编译/行为 RED、跨批次旧版本 RED、删除文件的打开身份 RED、溢出重新订阅 RED、新意图被拒绝后旧成功覆盖 RED，及对应 GREEN。主要最终日志为 `requests-final.log`、`editor-requests-final.log`、`request-workspace-tests.log`、`request-editor-tests.log`、`request-previous-contracts.log` 和 `request-consent-regression.log`。沿用前序串行编辑器验证，不据此宣称既有默认并行测试问题已解决。

## Standards

独立审查发现保存提交前缺少身份检查、静默重载未推进版本、磁盘删除被误报为文档关闭，共 3 项。全部修复，补充回归；最终复核 0 项遗留。主代理补充的拒绝新意图后旧成功覆盖问题也已复核通过。

## Spec

独立审查发现异步保存可能覆盖准备期间的外部变化、释放订阅后仍可能投递同批失败、订阅溢出后无法重新订阅，共 3 项。全部修复；最终复核 0 项遗留。主代理补充旧成功覆盖新拒绝的回归先 RED 后 GREEN，并经复核通过。

两个审查者只读审查，不重复运行 Cargo；表中测试均由主代理实际执行。未引入新进程、设置或组合 UI 能力，后续工单继续按既定依赖实施。
