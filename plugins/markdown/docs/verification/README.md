# Markdown 插件验收记录

01–11 的实现、验收、独立审查和 Git 交付均已完成；2026-10-04 最终独立读回 #27–#37 全部 closed / completed，父方案 #26 保持 open。各工单实际命令、结果、审查修正和限制分别记录。

- [01 — 语言包验证记录](01-language-package.md)（通过）。
- [02 — 原生预览验证记录](02-native-preview.md)（通过）。
- [03 — 视图模式验证记录](03-view-modes.md)（通过）。
- [04 — 格式工具栏验证记录](04-format-toolbar.md)（通过）。
- [05 — 图片预览验证记录](05-image-preview.md)（通过）。
- [06 — 图片粘贴与拖入验证记录](06-paste-drop-images.md)（通过）。
- [07 — 预览任务框验证记录](07-task-checkboxes.md)（通过）。
- [08 — 链接导航验证记录](08-link-navigation.md)（通过）。
- [09 — 代码块高亮验证记录](09-code-block-highlighting.md)（通过）。
- [10 — 双向同步滚动验证记录](10-synchronized-scroll.md)（通过）。
- [11 — 默认交付与完整组合验收](11-distribution-acceptance.md)（通过：正式 ZIP 完整 62/62、0 ignored；Git 推送和 #37 关闭已独立读回）。
- [12 — 主项目宿主集成与发行包验证](12-main-project-integration.md)（后续集成，记录主目录重新构建及新包验收）。
- [13 — 源码编辑区只有一行的布局修复](13-source-editor-height.md)（后续缺陷，记录实际输入高度、原生回归及宿主重建）。
- [14 — 图标工具栏与首次预览滚动回顶](14-icon-toolbar-and-first-scroll.md)（通过：18 项定向原生回归、64 项插件单元、SDK 与 workspace 门禁；交付 0.12.0）。
- [15 — GitHub 配色、六级标题、工具栏显隐与小幅滚动](15-github-headings-toolbar-scroll.md)（通过：22 项定向原生回归、65 项插件单元、公开接口及 SDK 验证、workspace 门禁；交付 0.13.0）。
- [16 — 预览表格文本、逐字重渲染与首次滚动回顶](16-preview-table-typing-scroll.md)（通过：表格按单元格原生渲染、输入不再撤回已发布预览并合并连续输入、手动预览位置优先于源码重排；交付 0.14.0）。

- [17 — 输入延迟、增量预览与滚动所有权](17-incremental-input-viewport.md)（0.15.0 自动化通过，但用户实测编辑区输入仍卡顿；输入性能验收重新打开）。
- [18 — 源码输入与过期视口通知](18-source-input-and-stale-viewport.md)（主项目宿主跟进；记录真实失败回归、输入测量口径及剩余人工验收）。
- [19 — 源码输入延迟的宿主根因（grammar 编译缓存）](19-source-input-grammar-cache.md)（宿主改动：插件 grammar 每次解析重新编译 WASM，release 实测 ~93 ms/次输入；共享带编译缓存的引擎后降至 2–7 ms，含新回归测试与失败归因；发行包待并发改动收敛后重建）。
- [20 — 底栏工具栏开关并入预览控件组](20-footer-toolbar-toggle-group.md)（宿主改动：工具栏开关与模式、同步开关同组同间隔（12 px → 3.5 px），非 Markdown 文件整组隐藏，偏好保留；含相邻间隔与文件切换回归）。

- [21 — 平台规格全文预览节点配额修复](21-platform-specification-preview-quota.md)（Markdown 0.17.0：实际原文完整预览和末尾滚动回归）。

- 每张工单完成后在本目录建立对应编号的验收文档，并从此处和[工单目录](../tickets/README.md)链接。
- 记录实际代码基线、包版本、命令、结果、原生可观察行为、失败归因、跳过项与平台限制。
- 覆盖范围对照[总方案验收矩阵](../spec.md)，不以其他插件历史验收替代 Markdown 的实际包验证。
