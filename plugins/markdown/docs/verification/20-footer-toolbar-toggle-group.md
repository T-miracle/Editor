# 底栏工具栏开关并入预览控件组

日期：2026-10-05。用户要求：显示/隐藏 Markdown 工具栏的按钮应与分栏、同步滚动按钮同属一组、间隔一致，并在切换到其他非 Markdown 文件时一起隐藏。本次改动位于主项目宿主；Markdown 包行为与版本保持 0.15.0，不改协议、不改访客。

## 缺陷与实测

- 旧实现把工具栏开关当作“面板开关”渲染在 `render_editor_preview_controls` 之后的独立 `h_flex().gap_1()` 里，而底栏本身用 `gap_3()`（12 px）分隔左右子项。实测同一帧内：`source→split` 3.5 px、`split→preview` 3.5 px、`preview→sync` 3.5 px，而 `sync→工具栏开关` 12 px，视觉上像另一组按钮。
- 非 Markdown 文件（`plain.txt`）下该按钮仍然显示：`active_editor_preview` 返回 `None`，但面板按钮列表只按“是否为带图标的编辑区预览”筛选，于是沿用上一次 Markdown 文档里的 `editor_toolbar_toggle` 文案继续显示一个 Markdown 专属开关。

## 修改

- `crates/editor-app/src/extensions/preview/presentation.rs`：工具栏开关并入 `render_editor_preview_controls` 的同一 `h_flex().gap_1()` 组，紧随同步开关，复用包图标与文档声明的 `editor_toolbar_toggle` 文案、`editor_preview_owner_key` 偏好键（与旧的 `{plugin}/{panel}` 键一致，用户已保存的显隐不丢）。组内没有任何控件可渲染时整组返回 `None`。
- `crates/editor-app/src/extensions.rs`：`plugin_panel_buttons` 跳过声明了 `editor_toolbar_toggle` 的编辑区预览面板，并删除其面板可见性分支；该按钮不再是面板开关，也不会在普通文件下退化显示。
- 调试选择器由 `plugin-panel-toggle-markdown/preview` 改为 `editor-preview-toolbar-toggle`（不再是面板开关）；`input_latency` 夹具同步更新。

## 验证

- `markdown_tests::icon_toolbar`（2 项）：断言 `source→split→preview→sync→toolbar` 四个相邻间隔与组内间隔一致（实测均 3.5 px，容差 1 px）；点击只隐藏顶部工具栏，预览正文与同步开关仍在；打开 `plain.txt` 后 `editor-preview-toolbar-toggle`、`editor-preview-sync-scroll`、`editor-preview-source-mode` 全部消失，且不再出现 `plugin-panel-toggle-markdown/preview`；切回 `notes.md` 后偏好保留（仍为隐藏）并可继续切换；深色主题下顺序不变。
- 相邻套件：`modes` 2、`format_toolbar` 4、`input_latency` 1、`distribution` 10、`combination` 1、`synchronized_scroll` 6、`task_checkboxes`（1 通过 3 失败，与本次无关，见下）全部按实际包运行。
- 失败归因：`task_checkboxes` 3 项、`link_navigation` 3 项在本轮之前即失败，失败断言属于预览任务框回写与链接打开计数；对应文件正由另一会话在工作区中改动，本次改动的临时旁路实验同样失败。

## 未验证与限制

- 非 Markdown 文件下不显示工具栏开关属于用户本轮明确要求；`plugins/markdown/AGENTS.md` 的旧表述“切换普通文件不改变其语义”已改写为“整组隐藏、偏好不变”，README 与总方案同步。
- 未做人工界面验收：断言基于原生 bounds 与文件切换，最终外观仍需在实际文件中目视确认（工具栏开关与分栏、同步按钮同组同间隔；`.txt` 下整组消失）。
- 发行包已重建并同步：`dist/editor/editor-app.exe` = `target/release/editor-app.exe`（95,479,808 字节，SHA-256 `2FA6CA0319FCB555FB9366AD643FDF946E369039872A4F7475B4604744A82532`）；Markdown 包未重新打包（本轮无插件改动，其源码仍在另一会话未收敛的改动中）。
