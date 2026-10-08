# 预览表格文本、逐字重渲染与首次滚动回顶

日期：2026-10-05。状态：三个缺陷已修复，定向原生回归、插件单元测试与仓库门禁通过；交付 Markdown 0.14.0 与配套宿主改动。

## 范围与依据

本记录对应用户本轮报告的三项缺陷，承接[记录 15](15-github-headings-toolbar-scroll.md)的当前行为，不改变其已验收结论。本轮不提交、推送或变更历史议题。

1. 预览表格中的 Markdown 文本未显示：单元格内的行内代码等富文本丢失字形，只留下底色方块。
2. 每输入一个字，整个编辑与预览区域被重新渲染。
3. 初始化加载 Markdown 文件后第一次滚动预览区，被强制回到预览顶部。

依据：[总方案](../spec.md)的 M02（常用语法，含表格）、M06（实时与过期结果）、M07（双向滚动）与[本插件 AI 约定](../../AGENTS.md)。

## 失败复现与定位

三项都在真实 ZIP → 公开 Manager → 原生输入的路径上复现，未修改访客私有状态，也未增加本插件专用宿主入口。

1. **表格文本**：用户截图的像素证据显示，除最后一行含行内代码的单元格外，其余单元格的底色矩形内部没有任何字形像素（颜色仅为底色 ± JPEG 噪声），即文字从未绘制。根因在宿主使用的上游富文本组件：整张表被插件生成为一个 `<table>` 富文本节点，而该节点的每个单元格段落都会走内联流布局，元素身份取自块级源码范围；HTML 解析不记录范围，于是同一节点内所有单元格共用同一个元素身份，只有最后一个能在那份共享状态中取回已排版文本，其余单元格按上游实现“跳过本帧”，底色仍由内联流单独绘制——正好是截图现象。插件原有的图片表格路径早因同样限制改为每单元格一个原生节点，本轮把它统一到所有 GFM 表格。
2. **逐字重渲染**：实测（临时探针，已移除）显示每输入一个字符会向访客发布一次完整文本，并在同一帧把该预览标记为未发布，于是 `current_document()` 返回空，预览区在访客回答前被整体撤回；访客回答后再整树重建。发布由渲染路径中的 `sync_plugin_panels → sync_editor_previews` 逐帧驱动，且每次编辑都会清空已发布 token 与代码高亮结果，因此每个字符都触发一次“撤回 → WASM 重解析 → 整树重建 → 重新高亮”。这是逐字重建的直接原因。
3. **首次滚动回顶**：源码侧仍在顶部时的重排通知会驱动预览侧对齐到第一个块。用户第一次滚动预览后，若该手动位置尚未送达访客（源码侧同帧更早发出、或请求仍在队列中），访客仍以“源码驱动”状态回发一次预览定位，宿主随即把预览滚回顶部。记录 14/15 已修同一源码版本刷新时的驱动方保留，但换 revision（输入）时仍会清空驱动方，且宿主侧未把“用户手动位置尚未确认”作为源码重排的前置条件。

## 修改

- 插件 `preview::Renderer::table`：所有 GFM 表格按行/单元格生成原生节点，每个单元格一个独立富文本块与独立身份，表头单元格使用通用 `github-muted` 预设获得 Primer muted 表面且内容加粗，每行后由宿主按主题边框色绘制分隔线；`apply_theme` 只补齐空 role，不再覆盖刻意声明的 role。
- 插件 `scrolling::Scrolling::invalidate`：同一文档的新 revision 只撤销在途请求，保留最近手动驱动的一侧；只有换文档才重新从顶部对齐。
- 宿主 `ExtensionPanel`：新增“在途发布”状态。同一文件的在途发布期间继续显示上一棵树（`displays_source` 按文档身份判断），连续输入合并为一次在途发布，访客回答后的下一帧补发最新文本；若访客连续 8 个 revision 未回答（例如通知被拒），按最新文本重发一次以免预览冻结。
- 宿主 `invalidate_editor_previews`：不再逐字清空已发布 token、代码高亮结果与源码视口测量。过期判断由文档身份与 revision 决定，新树到达时才撤销派生结果。
- 宿主源码视口上报：改为按“面板当前显示的树”上报（含其 source/revision），因此发布在途期间同步滚动仍对该树有效；显示树属于其他文件时依旧重置。
- 宿主预览视口：新增“手动位置尚未确认”标记。预览区滚轮（或滚动条按下）即标记，位置上报给访客后解除；该标记存在期间不上报源码重排，避免源码把已滚动的预览拉回顶部。滚轮取消在途定位不再依赖注册时捕获的 revision，因此一次重绘间隔内的滚轮同样生效。

## 公开边界与交付

- 未改协议版本、能力版本或 UI 文档版本：表格改为每个单元格一个 `ui::richtext` 节点，属既有公开能力；`github-muted` 为记录 15 已公开的通用 role。
- Markdown 清单、贡献、Cargo 版本统一为 0.14.0；`markdown.zip` 34 条目、22 个 SVG、3 份 WASM，SHA-256 为 `cddf304747a05feb79ae1395449586749dd158efee8a557b6059cd03f4d9dd8c`，`dist/plugins/bundle-defaults.json` 已同步该 hash。
- 表格外观随之变化：不再有整表外框与竖线，表头保留 muted 底色，行间为 1 px 分隔线。原因是整表外框只能由单个富文本节点提供，而该路径正是字形丢失的根因；公开 UI 协议目前没有通用容器的边框原语。

## 实际验证

定向 Windows 原生用例（全部显式 `--ignored`，安装交付的真实 ZIP，不绕过 Manager、DocumentSession 或原生输入）：

| 过滤词 | 通过数 | 覆盖 |
| --- | ---: | --- |
| `markdown_tests::preview_updates` | 2 | 新增：输入突发期间预览保持已发布且只产生一次在途发布、回答后补发最新文本；表格每单元格一个原生块并各自布局 |
| `markdown_tests::first_open` | 1 | 首次打开后不被同 revision 刷新拉回顶部 |
| `markdown_tests::small_scroll` | 2 | 1/2/5 px 手动滚动、亚像素源码位移、延迟不回弹 |
| `markdown_tests::code_highlighting` | 3 | 逐字不再清空高亮结果、提供者启停与迟到结果 |
| `markdown_tests::task_checkboxes` | 4 | 预览任务框写回与撤销 |
| `markdown_tests::synchronized_scroll` | 6 | 双向接管、偏移回执、图片/表格/换行/分栏重排 |
| `markdown_tests::format_toolbar` | 4 | H1～H6 与既有格式操作、撤销、键盘 |
| `markdown_tests::modes` | 2 | 三种视图与普通文件切换 |
| `markdown_tests::source_layout` | 2 | 实际源码高度及输入、缩放 |
| `markdown_tests::link_navigation` | 10 | 链接跳转、滚动事务与安全边界 |
| `markdown_tests::distribution` | 10 | 首次安装确认、禁用偏好保留、目录/hash 更换后的语义 |
| `delivered_markdown_preview_tracks_unsaved_native_edits` | 1 | 未保存编辑、粘贴、撤销、重做与磁盘重新加载的预览跟随 |

共 47 项通过、0 失败；日志见 `target/dsh-verify-*.log`。

宿主侧另有一条原生单元回归 `ui::plugin::viewport_tests::viewport_manual_input_withholds_source_reflow_until_it_is_reported`（新增），验证手动位置在报告前阻止源码重排、报告后解除。

插件独立单元测试：`target/debug/editor-app.exe --plugin-cargo plugins/markdown/Cargo.toml test --lib`，66 passed / 0 failed，含新增 `preview::tests::table_cells_keep_inline_code_in_independent_rich_blocks`。

仓库门禁全部 exit 0：`cargo fmt --check`；`cargo test -p editor-app -- --test-threads=1` 为 248 passed / 0 failed / 108 ignored；`cargo test --workspace --exclude editor-app -- --test-threads=1` 为 42 个测试目标、117 passed / 0 failed / 116 ignored；`cargo check --workspace` 通过。日志见 `target/dsh-gate-fmt.log`、`target/dsh-gate-workspace.log`、`target/dsh-gate-check.log`、`target/dsh-verify-editor-default.log`。

## 未验证与限制

- 仅 Windows 原生 TextAppContext 与真实包管线；未控制用户正在运行的编辑器窗口，其他操作系统未验收。
- 表格改为每单元格原生块后，逐单元格布局会随分栏宽度换行；未对超宽表格做横向滚动（记录 15 的 `small_scroll` 长表格用例仍通过）。
- 输入合并以“访客回答”为节拍：访客越慢，预览越合并；访客连续 8 个 revision 未回答时按最新文本重发，作为通知被拒后的自愈路径，未做时间型防抖。
- 发行目录已同步本轮宿主与插件：`dist/editor/editor-app.exe`（SHA-256 `962276d7a242d980daabdb65f0a8f724974a461ae34c87952a4af4c75e756a4e`）与 `dist/editor/plugins/markdown.zip`（与 `dist/plugins/markdown.zip` 同一 hash），两处 bundle catalog 均指向新 hash。重新打包目录不会自动升级用户已安装的旧插件，需经现有 Manager 安装/更新流程。
