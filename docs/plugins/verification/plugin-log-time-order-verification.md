# 插件运行日志本地日期与降序验证

状态：已完成，2026-10-04，Windows。依据：用户对运行日志截图的补充要求，以及[插件管理与运行日志规格](../specs/plugin-management-logs.md)。这是 #23–#25 交付后的修正，不修改历史工单的验收证据。

## 行为与边界

- 日志接收时间通过系统本地时区转换，固定显示 `YYYY-MM-DD HH:mm:ss`，补齐年月日及各时间字段，不显示毫秒或 UTC 后缀。完整日志和底栏摘要复用该格式。
- 完整日志按原始接收时间降序排列，原始时刻相同则按稳定记录 ID 降序；排序保留原始时间精度，不按已经格式化的字符串比较。
- 进入日志页的确认边界独立取捕获记录中的最大 ID，避免降序列表的最后一条变成最早 ID。并发新记录仍按实际可见情况读取，查看旧历史时不自动滚到新记录。
- 服务输出的原始正文保留，包括服务自己写入的 ISO 日期；本次格式要求作用于宿主展示的记录时间。运行期日志源、条数上限、提醒确认及权限契约不变。
- 使用锁文件中已有的 `chrono 0.4.45`，启用系统本地时间能力；`Cargo.lock` 仅增加 editor-app 的直接依赖关系，没有升级包版本。

## 回归验证

先在未修复实现上运行新增回归：

- 本地日历格式测试失败：原输出 `16:05:09.987 UTC`，预期 `2026-10-04 00:05:09`。修复后通过，并覆盖闰日、零点附近和省略小数秒；测试从本地日期构造时刻，不硬编码 UTC+8。
- 真实 GPUI 插件管理测试失败于“最新同秒记录应位于较早记录上方”；修复后通过。测试通过管理器共享日志源产生真实记录，点击插件及日志 Tab 后检查条目位置。
- 长日志测试通过：进入页面确认全部捕获记录；滚到旧历史后追加新错误，视图偏移不变，新条目在视口上方且仍未读；滚回顶部实际显示后才清除提醒。

验证使用 `HEAD 3d6d21cb7fb7a77a5f2eda349b0e44e95f7adab6` 加本次五份代码/依赖文件的隔离导出目录，与既有文档整理等工作区改动分离。所有 Cargo 命令串行执行：

```powershell
$env:CARGO_TARGET_DIR = 'C:/Projects/RustProjects/Editor/target/plugin-management-validation'
$env:RUST_MIN_STACK = '16777216'
$env:TEMP = 'C:/Projects/RustProjects/Editor/target/log-time-test-temp'
$env:TMP = $env:TEMP
cargo test --locked -p editor-app runtime_log_time_uses_local_calendar -- --test-threads=1
cargo test --locked -p editor-app extensions::management_tests:: -- --test-threads=1
cargo test --locked -p editor-app status_popover -- --test-threads=1
cargo test --locked -p editor-app extensions:: -- --test-threads=1
cargo fmt --check
cargo test --locked --workspace --exclude editor-app
cargo check --locked --workspace
cargo build --locked -p editor-app
```

本地日期 1 项、管理界面 6 项、底栏摘要 10 项通过；完整 extensions 回归为 31 通过、18 ignored。管理界面和日期测试包含在 extensions 合集中，不重复计数。非 UI workspace 为 78 通过、54 ignored；ignored 的真实 WASM/迁移用例本次未执行，不作为通过。本次没有修改 WASM、SDK 或包契约。格式检查、workspace 编译检查及宿主构建均通过。

底栏摘要回归覆盖长名称、长消息、大字号、卡片边界与键盘滚动；管理界面回归覆盖中文浅色、英文深色及 20 px 字号。只读复核确认本地日期、原始时间排序和独立确认边界相符。验证后五份代码/依赖文件与主工作区 SHA-256 全部一致。

测试首次遇到系统临时目录写入权限错误；改为本次进程专用的 `target/log-time-test-temp` 后重跑，复现到上述实际排序断言，后续回归通过。未修改系统权限或全局环境。

## 原生验收

使用新构建的宿主和既有独立声明式插件／LSP 夹具，经真实语言服务日志和原生点击、滚轮交互验收；没有操作用户当前运行的 release 编辑器。

- 中文浅色真实窗口：系统当前本地日期为 `2026-10-04 15:08:55`，底栏摘要和完整日志显示接收时间 `2026-10-04 15:08:56`，年月日完整、无小数秒和 UTC 后缀，摘要时间在卡片内完整显示。
- 点击摘要进入对应插件日志，最新警告在较早错误、初始化和启动记录上方。同秒显示标签一致仍保持实际接收先后。
- 点击固定顶部的重启按钮，产生 `15:10:11` 的新记录；新一轮日志位于顶部，滚动后可见其下方仍保留 `15:08:56` 的旧记录，顶部操作和 Tab 未滚走。
- 在旧历史位置再次重启，视图没有跳到新一轮顶部，日志 Tab 与底栏保留红色未读提醒。滚回顶部后显示 `15:11:29` 的最新记录，实际查看新消息后提醒消退，条目原始级别仍保留。

底栏和完整日志的真实日期排版、降序、共享提醒均通过。深色和大字号由上述真实 GPUI 交互测试覆盖，本次原生窗口未重新手工切换该两项。

## 限制

本次本地环境为 China Standard Time（UTC+8）；其他时区使用库的系统转换，未更改系统时区验收。macOS/Linux 未实测。历史 #24/#25 记录中的 UTC 示例保留为当时证据，当前显示规则以本文和更新后的规格为准。

`git diff --check` 通过，五份涉及的 Markdown 无行尾空白，57 个本地链接有效。本次新增的验收与规格链接均有效；主索引原有 Markdown 插件条目指向的 `plugins/markdown/docs/spec.md`、`plugins/markdown/docs/tickets/README.md`、`plugins/markdown/AGENTS.md` 当前不存在，这是既有文档工作区问题，未纳入本次时间／排序修复。
