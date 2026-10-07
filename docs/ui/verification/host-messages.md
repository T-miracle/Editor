# 宿主消息窗口实施验收

日期：2026-10-07

依据：[规格 #84](../specs/host-messages.md)、[两张工单](../tickets/host-messages/README.md)、[来源盘点](host-messages-sources.md)。

工作区：`C:/Users/Tmiracle/.codex/worktrees/host-messages/Editor`；分支 `codex/host-messages`；固定起点 `ae63225afd6e8a362c6f186d7374323e97405469`（主分支）。原工作区已有改动未混入。

## 工单 01 / #85

状态：实现、受影响回归、Windows 原生验收及 [双轴审查](host-messages-01-review.md)已完成；Standards 0 项，Spec 0 项。实现提交 `d182836`，推送至 `7f7097c` 后核对远程一致并关闭 #85，读回状态为 closed/completed。父规格 #84 保持 open。红点和清空属于 #86，不能把本阶段表述为整个功能完成。

实现沿 `EditorApp` 生产发布入口保留宿主结果，使用独立原生消息面板和本地 Base 控件，不增加插件契约。历史倒序、初始 20 条、逐批 20 条、共用 500 条上限；会话只保存布局、宽度及显隐。旧布局保留原树并追加消息叶；收起保留叶身份，关闭区域上的底部按钮一次打开。

### 自动回归

所有 Cargo 命令在上述工作区执行；使用 `CARGO_TARGET_DIR=C:/Projects/RustProjects/Editor/target` 和 `RUST_MIN_STACK=16777216`。原始日志存于工作区父目录，未纳入 Git。

| 命令 | 实际结果与范围 |
| --- | --- |
| `cargo test -p editor-app host_messages -- --test-threads=1` | 5 passed，0 failed，0 ignored；真实打开失败、真实编辑保存、六次运行准入拒绝、0/1/20/21/40/41 与 500/501/502 边界、实际滚动、停靠拖动、旧布局、受限工作区、隐藏及重启、Enter/Space、中英文与深浅主题及缩放 |
| `cargo test -p editor-app app::session::tests -- --test-threads=1` | 3 passed，0 failed，0 ignored；含原有异步插件布局恢复。仅为旧夹具的新增消息叶调整期望，保留原有尺寸舍入比较与原树/关闭状态断言 |
| `cargo test -p editor-app status_tests -- --test-threads=1` | 10 passed；通过真实公共管理器安装的包发布含宿主代报错误的插件日志，宿主窗口仍为空；插件提醒原有回归通过 |
| `cargo test -p editor-app management_tests -- --test-threads=1` | 5 passed；插件完整日志与清空边界原有回归通过 |
| `cargo test -p editor-app dock_tests -- --test-threads=1` | 1 passed；插件原生停靠拖动与尺寸保存通过 |
| `cargo test -p editor-app run::run_ui_tests -- --test-threads=1` | 18 passed；受影响运行界面与宿主拒绝回归通过 |
| `cargo test -p editor-app explorer_selection_tests -- --test-threads=1` | 2 passed；文件树与实际 tab 激活回归通过 |
| `cargo test -p editor-app editor::file_watch -- --test-threads=1` | 4 passed；磁盘变化与后台树刷新回归通过 |
| `cargo fmt --check` | 通过 |
| `cargo test --workspace --exclude editor-app` | 通过；WASM 夹具相关 ignored 测试未执行，不计作通过 |
| `cargo check --workspace` | 通过 |
| `cargo build -p editor-app` | 通过；既有未使用代码及链接器警告仍存在 |

TDD 红阶段先在真实窗口缺少消息或无法恢复的可见断言失败，再补实现转绿；包括关闭区域的一次恢复和真实运行准入拒绝。键盘测试使用完整 KeyDown/KeyUp，避免把只发送按下的测试输入误报成控件故障。

### Windows 原生交互

通过 `computer-use` 在隔离目录 `C:/Users/Tmiracle/.codex/worktrees/host-messages/acceptance` 启动本次构建的专用副本 `host-messages-acceptance.exe`。插件目录通过 `ME_EDITOR_PLUGIN_HOME` 隔离；只操作该绝对路径的验收实例，没有操作用户已有编辑器窗口。

- 默认右侧显示，标题及底部入口使用指定通知 SVG。
- 对临时真实文档使用 Ctrl+S，完成的“没有未保存的更改”进入历史；连续 20 次产生超过首批容量的结果。
- 点击收起释放编辑宽度；点击底部通知按钮恢复已有记录。
- 实际拖动右停靠边界，将约 320 px 调至 400 px；滚动至底部后点击“查看更多...”，追加最后一条，按钮消失。
- 通过原生设置切换深色主题，消息标签、正文和通知图标可辨识。
- 收起并关闭隔离实例，用最终阶段构建重启；确认仍隐藏，一次点击恢复约 400 px 宽度，显示“暂无宿主消息”，没有旧记录。

重启验证副本 SHA256：`0425CECB2F4BB6AF88C5E684B4C3C2E962EC4779030696C08F88C5713CEEF2D6`。原生键盘恢复、语言切换及缩放另由统一 GPUI 输入回归证明；没有把未操作的原生场景报为手工通过。

## 工单 02 / #86

状态：#85 已交付；本单实现、自动回归、Windows 原生组合验收及 [最终双轴审查](host-messages-02-review.md) 已完成，Standards 0 项，Spec 0 项。实现提交 `4ac35d9`，推送至 `08101f7` 后核对远程一致并关闭 #86，读回状态为 closed/completed，11 项验收勾选及完整正文一致。父规格 #84 保持 open。最终全量 editor-app 测试集中在此阶段执行一次，未受影响的手工场景引用工单 01 的记录。

红点由新警告或错误设置，只由用户打开或清空同步确认；普通消息、重绘、展开与淘汰均不能确认提醒。清空重置当前历史、提醒、展开数量及滚动位置，保留单调消息身份供后续记录使用。插件日志和提醒没有共享读写路径。

### 自动回归

沿用工单 01 的应用级夹具；提醒、清空各先在真实可见行为断言失败，再补实现转绿。提醒回归经过真实文件打开失败和实际按钮，故意在打开后的延迟重绘前送入新警告；清空回归覆盖未展开历史、淘汰全部旧异常但保留提醒、清空后立即到达、重新按 20 条展开，以及 Enter/Space 激活。既有重启夹具只增加旧红点不恢复断言，没有复制布局测试。

| 命令 | 实际结果与范围 |
| --- | --- |
| `cargo test -p editor-app host_messages -- --test-threads=1` | 8 passed，0 failed，0 ignored；包含六项共用窗口回归、真实运行准入来源回归，以及通过公共管理器安装两个声明式插件包的日志、已读、提醒隔离回归 |
| `cargo test -p editor-app -- --test-threads=1` | 373 passed，0 failed，142 ignored；全量执行一次，包含上述八项及既有文档、布局、插件日志、设置和控件回归。142 项需要独立 WASM/外部夹具的测试未执行，不计作通过 |
| `cargo fmt --check` | 通过 |
| `cargo test --workspace --exclude editor-app` | 通过；原有 ignored 夹具测试未执行 |
| `cargo check --workspace` | 通过 |
| `cargo build -p editor-app` | 通过；既有未使用代码和链接器警告仍存在 |

原始日志：工作区父目录下 `reminder-red.log`、`reminder-green.log`、`clear-red.log`、`clear-green.log`、`stage-two-host-tests.log`、`final-ui-tests.log`、`stage-two-workspace-test.log`、`stage-two-check.log` 和 `stage-two-build.log`。未修改插件协议、WASM 包或 SDK 分发，没有把夹具缺失解释为对应测试通过。

附图源文复核：仓库两份 SVG 与原附件的 XML 全文一致，只增加了文件末尾换行。初次逐字节 SHA256 校验因这一格式差异失败，去掉末尾换行后按 ordinal 比较通过；图形路径与属性没有改变。

### Windows 原生组合验收

使用与工单 01 相同的隔离目录和专用可执行副本，只操作返回身份与绝对路径一致的验收窗口。最终副本 SHA256：`75D0E49035330C0CCB031E2444F356602F81E5A08C901D6F9E9DF3F94772E05A`。

1. 收起消息窗口，在临时文档中实际输入，点击未保存 tab 的关闭按钮；关页保护产生宿主警告，窗口保持收起，底部通知图标出现红色圆点。
2. 点击底部按钮，已有红点消失，警告保留，右侧恢复约 400 px 宽度。实际保存成功及 19 次没有修改的 Ctrl+S 形成共 21 条历史，普通结果不产生新红点。
3. 滚动到底部，点击“查看更多...”追加剩余警告，入口消失；点击“清空消息”，显示“暂无宿主消息”，红点与查看更多均不存在。
4. 清空后再次实际输入并点击关页，窗口已显示时新警告仍点亮红点。通过设置切换深色主题，红点、通知图标和清空按钮可辨识，主题结果没有清除红点。
5. 重新核对窗口后，实际收起再打开，提醒被确认而警告仍在；再次产生警告并保存，普通保存结果未清除提醒。收起并正常关闭隔离实例，重启后仍隐藏、无旧红点，一次打开恢复约 400 px 宽度并显示空历史。

自动化操作说明：首次关闭设置使用含相关弹窗的主截图坐标后，验收实例退出，未取得该次设置关闭的通过证据。重启后改用设置弹窗自身的截图点击关闭，主窗口保持存活、stderr 为空，再完成上述收起、确认和正常关闭重启验证。没有据此宣称未核实的崩溃原因，也没有把设置关闭混作消息清空。

中英文、Enter/Space 和 1.5 倍缩放由 GPUI 实际输入回归覆盖；原生手工覆盖中文、深浅主题、滚动、展开、清空、实际编辑与重启，没有把未手工操作的语言或缩放场景标为手工通过。

## 最终规格覆盖

矩阵中的 UI 回归来自 [共用窗口测试](../../../crates/editor-app/src/tests/host_messages.rs)，插件隔离来自 [日志入口测试](../../../crates/editor-app/src/app/plugins/status_tests/host_messages.rs)，来源归属见 [来源盘点](host-messages-sources.md)。工单 01 手工记录仅用于本单没有改变的场景，相关自动回归在最终版本已重新执行。

| 场景 | 证据 |
| --- | --- |
| M01 | `host_messages_record_file_failure_and_reopen_without_losing_history` 的默认右侧占位；工单 01 原生启动 |
| M02 | 真实打开失败与实际编辑保存回归、真实运行准入守卫、来源盘点；本单原生关页警告及保存 |
| M03 | `host_messages_confirmation_and_clear_preserve_plugin_logs`：两个实际安装包的插件警告、宿主代报插件错误不进入宿主；确认和清空后插件记录、已读等级及提醒不变 |
| M04 | `host_messages_progressive_history_keeps_the_latest_five_hundred`：0/1/20/21/40/41 与实际按钮；本单原生 21 条展开 |
| M05 | 同一生产入口连续接收，逐条断言倒序可见位置和身份，不按时间戳猜顺序 |
| M06 | 混合等级共用 500 条，501/502 淘汰最早记录；最终自动回归通过 |
| M07 | 已展开后的新到及淘汰；本单清空回归进一步证明展开和淘汰都不确认红点 |
| M08 | 实际收起释放宽度、按钮恢复已有记录；本单原生收起和确认 |
| M09 | 共用实际拖动与重启回归，增加旧红点不恢复；本单正常关闭重启恢复隐藏和约 400 px，历史、红点为空 |
| M10 | `host_messages_extend_legacy_layout_and_support_native_keyboard` 保留右侧旧 peer；既有异步插件布局恢复回归通过 |
| M11 | 真实保存结果不触发或清除红点，面板收起与编辑焦点保持；本单原生保存 |
| M12 | 真实打开错误和新警告触发圆点；可见及隐藏状态均不抢编辑焦点；本单原生关页警告 |
| M13 | 真实底部按钮确认，错误记录保留，无需展开；本单原生打开 |
| M14 | 打开激活后、延迟重绘前的新警告仍提醒；再次绘制和普通消息不确认；本单原生已显示窗口的新警告 |
| M15 | 实际清空按钮清除所有历史、红点和展开入口，新到警告仍可见；插件隔离回归；本单原生清空后再接收 |
| M16 | Base 控件 Enter/Space、中英文资源、深浅主题、滚动、1.5 倍缩放及 20 px 字号回归；原生中文主题、图标、红点和新增按钮可辨识 |
| M17 | 受限旧工作区内实际按钮、消息发布与布局恢复可用；无需启动插件或语言工具 |

## 交付与跟踪器读回

- 交付分支：`codex/host-messages`，从主分支固定起点建立的新工作区；两阶段实现提交分别为 `d182836`、`4ac35d9`。
- #85 在推送并核对 `7f7097c` 后关闭；#86 在推送并核对 `08101f7` 后关闭（2026-10-07T10:58:20Z）。两单状态均已从 GitHub 读回为 closed/completed。
- GitHub 连接器可读取议题，但写入返回 `403 Resource not accessible by integration`；按议题跟踪器约定，使用现有 Git 认证调用 REST 更新 #86 的完整验收正文并关闭，再核对完整正文与关闭状态。凭据未写入文件或输出。
- 父规格 #84 已读回确认 open，未修改。真实议题身份、实施与关闭时推送 SHA 见 [发布记录](../tickets/host-messages/publication.json)。
