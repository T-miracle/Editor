# 宿主消息窗口实施验收

日期：2026-10-07

依据：[规格 #84](../specs/host-messages.md)、[两张工单](../tickets/host-messages/README.md)、[来源盘点](host-messages-sources.md)。

工作区：`C:/Users/Tmiracle/.codex/worktrees/host-messages/Editor`；分支 `codex/host-messages`；固定起点 `ae63225afd6e8a362c6f186d7374323e97405469`（主分支）。原工作区已有改动未混入。

## 工单 01 / #85

状态：实现、受影响回归、Windows 原生验收及 [双轴审查](host-messages-01-review.md)已完成；Standards 0 项，Spec 0 项。待推送与关闭 #85。红点和清空属于 #86，不能把本阶段表述为整个功能完成。

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

状态：等待 #85 交付；尚未开始红点和清空实现。最终全量 editor-app 测试与跨功能组合验收集中在此阶段执行一次，未受影响的手工场景引用工单 01 的记录。
