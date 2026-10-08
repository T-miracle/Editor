# 输入延迟、增量预览与滚动所有权

日期：2026-10-05。对应用户明确批准的增量实施；本记录不关闭历史工单、不提交或推送。代码、公开接口与原生自动化回归通过，交付 Markdown 0.15.0 和配套宿主。

**验收修正：用户在配套 0.15.0 中仍然复现 Markdown 编辑区域输入卡顿，输入性能问题重新打开。下述 draw 测量只证明绘制阶段改善，不能证明按键、中文输入及文档更新的完整链路已经解决；原先“输入卡顿已修复”的结论撤回，继续测量输入到文字显示的延迟。**

## 需求与边界

- 从超过两屏的普通 Markdown 起验收，包含中文输入；仅编辑模式也不能卡顿。
- 输入优先，首次编辑后约 150 ms 合并正文更新，连续输入仍定期发布最新快照。
- 普通段落修改只解析受影响块，未变化块复用；结构边界、引用定义、代码围栏等可能改变远处语义时扩大解析范围。
- 预览仅布局可见区域和缓冲区；未测量区域使用高度估计，滚动/导航到达时测量，以块锚点保持位置。
- 最后实际操作的一侧控制同步；迟到的同步请求不能抢回控制权。仅编辑模式暂停正文派生，工具栏仍通过当前文档授权执行。

## 可复现证据与归因

真实包经公开 Manager 安装，70 段普通中文文档。最初使用 `simulate_input` 得到约 190 ms，但该助手会排空后台执行器，因此不能视为输入线程耗时。改用直接按键派发并计时 `window.draw` 后，工具栏可见的单帧约 63 ms，隐藏后约 4 ms；按钮树构造、布局请求、prepaint、paint 合计约 2 ms。明确紧凑图标按钮的外层 24 px 尺寸后，相同单帧约 17 ms。瓶颈是嵌套自适应换行布局的尺寸计算，关闭语法解析没有改善；未修改或复制上游高亮实现。

日志：`target/markdown-perf-bodies.log`、`target/markdown-perf-toolbar.log`、`target/markdown-perf-toolbar-element.log`、`target/markdown-perf-icon-extent.log`。临时插桩已移除，保留真实包的单帧性能回归测试。

滚动红灯：`cargo test -p editor-app viewport_late_source_locate_cannot_reverse_a_reported_wheel -- --nocapture`。物理预览滚轮位置已上报后，再送达旧源码定位请求，原实现把 -60 px 拉回 0 px（`target/markdown-incremental-wheel-red.log`）。修复后请求取消、可见位置保持；测试不依赖仅扩大像素阈值或定时锁。

## 实现与公开边界

- 宿主按原生实际输入保留源/预览所有权，另保留程序回执；显式链接导航可以接管。无插件 ID 或 Markdown 扩展名业务分支。
- `editor.presentation 1.1` 提供预览显隐通知与合并节拍。源工具栏点击可提前刷新快照；等待期间绑定实例、文档 revision 与完整按钮定义，后续输入或换文档取消点击。
- `ui.incremental 1.0` 的 `ViewPatch` 只引用同实例、同面板精确基线中的旧子树。宿主恢复后继续执行完整权限、来源、节点及字节预算验证，失败不部分发布。
- Markdown 的只读解析缓存保留稳定节点 ID；普通文本编辑复用其余节点并平移字节范围，链接索引也局部更新；标题变化重算去重锚点。源码、选区、IME 和撤销仍由 EditorState/DocumentSession 唯一管理。
- 通用原生块虚拟布局保存派生高度；宽度、主题、图片尺寸变更使测量失效，远处导航强制实现目标块的真实布局。当前窗口化单位是 Scroll 内的顶层映射块（至少 32 块），含链接或控件的块保留完整键盘遍历；单个大表格/代码块内部不再细分。这不是任意大小文档的无限虚拟化，原有 1 MiB/节点预算仍有效。
- 隐藏预览发布空正文，保留插件内部只读解析缓存。这样源码变短后，旧正文的字节范围不会错误地附着到新的工具栏文档授权上；显示时恢复当前正文。

## 回归发现与修正

- 源码快捷键会在控件 KeyDown 之前被 GPUI action 消费。通过窗口级 `intercept_keystrokes` 和当前编辑器焦点检查记录实际输入，不阻止原生按键处理；Ctrl+Home 重新接管预览后，真实链接点击成功。
- 预览物理滚动同帧触发惰性测量时仍报告 manual，不能当作布局回执；预览点击的全局 MouseUp 不再被误认为源码拖拽释放。
- 工具栏刷新原先在 PluginView 输入回调内重入同一实体。改为 defer，随后复核实例、文档版本与按钮定义，“输入中文后立即点 H1”通过。
- 原有测试把零位移的接管与后续真实滚轮合计当成一个事件；现在分别验证两次真实意图均只上报一次。另一项旧测试补上显式源码接管，再验证它能覆盖此前链接导航。
- 隐藏期间不暴露旧源码范围；窄预览按实际宽度缓存块高度，只有首次尚未布局时才使用默认宽度。

## 实际验证

| 检查 | 结果与日志 |
| --- | --- |
| `cargo fmt --check`、`git diff --check` | 通过；`target/markdown-format-final.log`、`target/markdown-diff-check.log` |
| `cargo test --workspace --exclude editor-app` | 120 通过，118 ignored；跳过项不算通过。`target/markdown-workspace-tests.log` |
| `cargo check --workspace` | 通过；`target/markdown-workspace-check.log` |
| `cargo test -p plugin-runtime --test incremental_ui -- --ignored --nocapture` | 真实 ZIP 安装、完整树恢复、隐藏解析与能力拒绝 2/2；`target/markdown-runtime-delta.log` |
| `editor-app.exe --plugin-cargo plugins/markdown/Cargo.toml test` | 69/69；含增量/全量语义对照及 100 段复用 99 段。`target/markdown-guest-final.log` |
| `cargo test -p editor-app --bin editor-app viewport_tests -- --nocapture` | 最终 13/13；`target/markdown-viewport-delivery.log` |
| 原生真实包定向分组（下述命令） | 共 29 个不同相关用例最终通过；包含上述 13 个视口用例。未声称整个 editor-app 或全部 Markdown 用例通过 |
| `./scripts/verify-plugin-sdk.ps1` | 仓库外独立构建、导出修复通过；`target/markdown-sdk-final.log` |
| `cargo build -p editor-app --release` 和 Markdown 打包 | 发行宿主与 0.15.0 ZIP；`target/markdown-release-delivery.log`、`target/markdown-release-package.log` |

原生分组使用 `cargo test -p editor-app --bin editor-app -- --include-ignored --nocapture --test-threads=1`，筛选组如下，保留中间失败及收敛证据：

1. `viewport_tests responsiveness combination::delivered_markdown delivered_markdown_preview_tracks_unsaved preview_updates small_scroll synchronized_scroll`：28 项，首次 25 通过、3 失败，见 `target/markdown-focused-final.log`。
2. `responsiveness combination::delivered_markdown viewport_manual_input_withholds`：新增短反馈 Home 用例后 6 项；工具栏、输入性能、滚轮和零位移事件已通过，两个 Home 相关失败定位到快捷键接管，见 `target/markdown-review-native.log`。
3. `source_home_reclaims combination::delivered_markdown viewport_tests`：修正按键监听后 15/15，见 `target/markdown-navigation-final.log`。最终视口组再通过 13/13。

可见行为覆盖单格 Lines 滚轮、1/2/5 像素位移、大图、换行表格、双向定位、关闭同步、来源退休、中文预编辑、撤销重做、图片插入、任务框、链接、主题和字号/窗口缩放。最终六次 native draw：仅编辑模式 16.2–17.1 ms，分栏模式 8.3–8.7 ms；这是调试构建的绘制测量，不把排空后台任务的测试总耗时冒充输入延迟。性能日志在 `target/markdown-review-native.log`。

曾启动全部 Markdown 用例以探索回归，发现失败后中止并切换为上述定向闭环；没有将中止或 skipped 计为通过。现有 Windows LNK4217、未使用成员等警告仍存在。原生自动化调用真实 GPUI 布局、键鼠事件与 IME 接口，但没有人工操作硬件鼠标或 Windows 输入法候选窗。

## 使用交付

使用 `dist/editor/editor-app.exe`，并在插件管理中更新/安装同目录 `plugins/markdown.zip`（0.15.0）。宿主与包必须配套；用户已有安装、禁用及卸载选择不会被静默覆盖。版本、路径和随包 SHA-256 索引在发行拷贝后读回核对。

2026-10-05 13:39（本机时间）读回：宿主与 `target/release/editor-app.exe` 相同，包清单为 `markdown / 0.15.0 / protocol 7`，同目录 `bundle-defaults.json` 的包哈希匹配。

| 产物 | SHA-256 |
| --- | --- |
| `dist/editor/editor-app.exe` | `AA1ACF97AF5AD2934F2DEB568CBD3D3A5921C6AB0F2041E777E506B99A146D07` |
| `dist/editor/plugins/markdown.zip` | `54E52ED9399FCEA048E7E892C5F989B427B9D38B51F781ADB49EBAE18393C856` |
