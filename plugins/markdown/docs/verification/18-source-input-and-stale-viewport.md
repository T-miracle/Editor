# 0.15.0 后续：源码输入与过期视口通知

日期：2026-10-05。用户确认配套 0.15.0 仍有源码输入卡顿，并提供 `StaleRevision: Viewport source or scene has changed`、来源 `host.operation` 的截图。本次修复在主项目宿主；Markdown 包保持 0.15.0，不改协议和访客行为，不提交、推送或关闭历史工单。

## 可复现证据

- `cargo test -p editor-app --bin editor-app delivered_markdown_input_to_frame_latency -- --ignored --nocapture`：真实包经 Manager 安装，使用 `crates/plugin-protocol/UI.md` 前 120 行（中文、代码、表格与长行）。直接派发原生按键，计时包含同步 effects，再显式完成一帧；后台排空单独计时。原始测试失败：分栏约 155 ms。进一步显式启用已发布的插件 grammar 后，对照基线为纯文本约 12.6 ms、仅编辑约 40.1 ms、分栏约 156.9 ms；关闭高亮仍约 136.8 ms，不能把主要耗时归因于高亮。日志：`target/markdown-input-latency-baseline.log`、`target/markdown-input-highlight-control.log`。
- `cargo test -p editor-app --bin editor-app stale_source_viewport_is_discarded_without_plugin_error -- --ignored --nocapture`：真实生产 worker 和交付包先发布 revision 2，再收到 revision 1 的源码位置。旧实现产生用户可见的操作错误，回归失败。日志：`target/markdown-stale-baseline.log`。首次失败断言位于共享状态锁内，还引发测试清理时的锁中毒；测试已改为释放锁后断言，不能把该清理错误当作产品崩溃。

## 修改与边界

- `extensions/surface.rs` 将原生插件场景放入占满既定视口的绝对定位容器，让预览内容不再参与祖先容器的固有尺寸计算。场景继续正常布局、绘制和注册交互回调，没有保留旧交互树，没有增加 Markdown 专属宿主分支，也没有缓存第二份源码。
- `extensions/preview/viewport.rs` 在源码 revision 与当前显示的预览 revision 不一致时暂停源码位置采样，不再给新文本的字节位置标上旧文档版本。保留用户滚动所有权；预览追上后继续同步。
- `extensions/worker/runner.rs` 将 `SourceViewport` 纳入原生过期回调处理：Manager 仍严格拒绝过期位置，仅将该类 typed `StaleRevision` 记为 `host.ui.stale` 信息，不产生错误状态。非法参数、权限错误和访客故障仍走错误路径；没有修改 Manager 的版本校验。

## 输入测量的含义

最终同一调试构建测试的中位数如下；前一列包含按键派发及同步 effects，后一列还包含随后显式绘制一帧，所以不能把两列都称为单帧绘制时间。

| 模式 | 派发及同步 effects | 加显式绘制一帧 |
| --- | ---: | ---: |
| 同文纯文本 | 6.2 ms | 12.2 ms |
| Markdown 仅编辑 | 21.1 ms | 39.2 ms |
| Markdown 分栏 | 43.7 ms | 84.4 ms |
| Markdown 仅编辑、隐藏工具栏 | 10.7 ms | 18.4 ms |
| Markdown 分栏、隐藏工具栏 | 27.8 ms | 52.8 ms |

日志：`target/markdown-input-bounded-final.log`。修改主要降低了分栏输入的同步布局开销；仅编辑仍比纯文本慢，不能宣称所有输入卡顿已完全消失。中文样本为原生字符按键，不等于操作系统中文输入法候选窗人工验收。原生 UI 测试手动驱动 Manager 通道，不能替代生产 worker 与物理输入同时运行的端到端耗时；过期通知测试单独使用真实 worker。

## 扩展检查发现的导航失败

`markdown_tests::link_navigation` 为 7 通过、3 失败：两个同步导航测试把 `LocateViewport` 和 `NavigateDocument` 都计入 `requests.len()`，实际仍只有一个打开请求；另一个远处链接的键盘焦点提示底部超出视口。恢复原有 `.child(view)` 布局后，三个失败及焦点几何完全相同，因此不能归因为新的尺寸布局方案，也不能把这组检查记为全部通过。当前请求不修改这两处测试断言或独立的焦点提示行为。

日志：`target/recheck-markdown_tests-link_navigation.log`（缓存候选）、`target/bounded-markdown_tests-link_navigation.log`（最终容器方案）、`target/markdown-links-original-layout.log`（恢复原预览布局的对照）。曾将这组失败口头归因于缓存方案，后由原布局对照纠正。最终选择不缓存交互树的定尺寸容器；临时请求打印已移除。

## 验证与产物

| 检查 | 实际结果 |
| --- | --- |
| 最终原生交互组 | 36 通过、0 失败、0 ignored；包含真实 worker 的过期通知、源码版本、两侧滚动、首次打开、源区高度、Markdown/SVG 模式、撤销与工具栏检查。`target/markdown-followup-final-native.log` |
| 混合文档输入性能 | 单独运行 1 通过，避免与其他测试争用 CPU；数值见上表。`target/markdown-input-bounded-final.log` |
| 输入后立即滚动 | 追加 1 通过；在旧预览上先滚动，再补发新文本，最终位置保留。`target/markdown-pending-edit-wheel.log` |
| `cargo test --workspace --exclude editor-app` | 120 通过，118 ignored；跳过项不算通过。`target/markdown-followup-workspace-tests.log` |
| `cargo check --workspace` | 通过。`target/markdown-followup-check.log` |
| `cargo fmt --check`、`git diff --check` | 通过。`target/markdown-followup-format.log`、`target/markdown-followup-diff.log` |
| `cargo build -p editor-app --release` | 通过，Windows 既有链接器与未使用代码警告仍保留。`target/markdown-followup-release.log` |
| 导航扩展检查 | 7 通过、3 失败；恢复原预览布局后结果相同，详情见上一节，不纳入上述 38 项通过数。 |

原生组复现命令：

```powershell
cargo test -p editor-app --bin editor-app -- --include-ignored --nocapture --test-threads=4 markdown_tests::preview_updates markdown_tests::small_scroll markdown_tests::first_open markdown_tests::synchronized_scroll markdown_tests::responsiveness markdown_tests::source_layout markdown_tests::icon_toolbar markdown_tests::modes ui::plugin::viewport_tests stale_source_viewport_is_discarded_without_plugin_error
```

当时此组为 36 项；随后新增的 pending-edit 滚动用例单独运行通过，因此现在重跑该组会包含 37 项。性能测试另行执行，不与其他重型原生夹具同时测量。

宿主生成时间为 **2026-10-05 14:13:54**（本地时间）。已更新 `target/release/editor-app.exe` 和 `dist/editor/editor-app.exe`，两份 SHA256 均为 `CCD81DE6FDD1D084DE82766910773C1ACE0F65D30475B618ABB0F6FCF5ED79FF`。现有 `dist/editor/plugins/markdown.zip` 仍为 0.15.0，SHA256 保持 `54E52ED9399FCEA048E7E892C5F989B427B9D38B51F781ADB49EBAE18393C856`；本次无需重装插件。用户已保存并退出运行中的宿主后才构建和替换 EXE，未强行终止进程。

输入性能问题仍保留实际文件、物理键盘和操作系统输入法的人工验收项，不重复此前以局部自动化结果宣称全部修复的结论。未进行 Git 提交、推送或工单关闭。
