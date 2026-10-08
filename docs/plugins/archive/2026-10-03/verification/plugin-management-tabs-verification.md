# 插件管理分栏验证（工单 01 / GitHub #23）

状态：已完成实现与验收，2026-10-03，Windows。规格：[插件管理与运行日志改造](../specs/plugin-management-logs.md)，[实施工单](../tickets/plugin-management-logs/01-management-tabs.md)。本记录只覆盖 #23；#24–#25 尚未交付。

## 交付行为

- 左右两组 Tab 共用本地下划线控件，基于锁定的 gpui-base 0.7.0。右侧顺序为概览、最新变化、评价、其他版本、其他信息、运行日志；中间四项置灰且禁止鼠标和键盘激活。方向键环绕可用项，Home / End 可定位；焦点组显示主题颜色边线。
- 名称、版本、操作和 Tab 保持固定，只有其下方内容由独立 ScrollHandle 滚动。切换插件、列表类别或详情 Tab 时重置详情滚动位置；选择插件、关闭后重新打开管理窗口均恢复默认概览。
- README 留在概览，已有服务状态、当前诊断与归属于该插件的操作错误进入运行日志。发布通道保留错误插件身份，切换插件不会串入上一个插件的错误；无法归属插件的管理器故障在独立区域呈现，即使没有插件也可见。
- 重启是顶部普通宽度按钮，位于全局选项之前，继续走原生命周期工作线程。安装、更新、卸载及启用范围的条件和权限保持。操作行可换行，全局选项随界面字号增宽；Tab 标题在窄窗口下省略，悬停及无障碍标签保留完整标题。
- 左侧搜索与 Tab 的间距由 gap_3 缩至 gap_2；底部安装区域由 p_3 调整为 px_2 / py_1。新增可见文本保留中英文及主题颜色。

## 验证证据

```powershell
$env:RUST_MIN_STACK = '16777216'
cargo test -p editor-app management_tests -- --test-threads=1
cargo test -p editor-app extensions::tests -- --test-threads=1
cargo test -p editor-app operation_errors_retain_plugin_ownership_through_worker_publication -- --test-threads=1
cargo build -p editor-app
./scripts/build-capability-example.ps1
cargo test -p editor-app native_restart_recovers_a_fault_without_blocking_document_input -- --ignored --test-threads=1
cargo test -p editor-app cancelled_preparation_does_not_replay_an_older_trust_grant -- --ignored --test-threads=1
cargo fmt --check
cargo test --workspace --exclude editor-app
cargo check --workspace
git diff --check
```

- 新增的 3 项管理页测试从真实声明式 ZIP 经 Package / Manager 安装进入后台发布和原生管理窗口，覆盖中文浅色、英文深色与关闭后重新打开。验证长文滚轮实际改变内容偏移而固定区域 bounds 不变、占位 Tab 点击无效、键盘跳过占位项、说明与运行信息分开、切换插件不串错、重启消息到达工作线程，以及 850 × 600 窗口、20px 字号的操作区域边界。
- 原有 7 项管理相关测试通过，覆盖安装加载、第二包确认、卸载数据选项与不兼容包的更新及范围保留。
- 生产 actor 的普通测试验证插件命令失败携带目标身份、无法确定目标的 ZIP 检查失败保留管理器身份。
- 构建实际 WASM 夹具后显式执行故障恢复测试：故障诊断只在日志页出现，顶部重启使用相同公开管理器恢复实例，未保存编辑内容保留。取消候选准备后不回放旧信任授权的 actor 回归也通过；不是将 ignored 用例跳过计为通过。
- Windows 实际窗口检查使用独立测试工作区、独立插件根和应用副本。中文浅色窗口中确认六个 Tab、较紧凑侧栏、正常宽度重启及项目选项；滚动长说明后名称、操作、Tab 不移动，点击日志页后正文切换，Left 返回概览并跳过四个占位项。界面工具随后检测用户按 Esc，停止后续界面操作。英文、深色及字号放大由 GPUI 交互与布局测试覆盖；不宣称这些模式都有桌面截图验收。
- 阶段格式、非 UI workspace 测试、workspace 编译及 diff 检查通过。提交候选单独导出后复验相关交互与生产 actor 测试，避免依赖其他未提交变更。

## 审查与边界

独立规范和界面审查指出错误归属、英文全局选项宽度及键盘焦点提示问题，均已修正并由主代理复核；子代理随后因额度限制停止，剩余验证由主代理完成。

目前日志仍显示既有的当前服务状态、最新有界诊断与一次操作错误，不是完整运行历史。完整日志、未读严重程度图标、过期 UI 事件的提醒策略属于 #24；底栏持续提醒、A 方案及定位联动属于 #25。没有升级公开协议、SDK 或插件包。

既有 CollectionModel 可见性、未使用辅助方法与 Windows wasmtime / tree-sitter 链接告警仍存在。未执行与本切片无关的全部 ignored 验收，不将其列为本次通过。
