# 21 — 平台规格全文预览误触节点配额

日期：2026-10-06。Markdown 包版本：0.17.0。针对用户报告的 `docs/plugins/specs/plugin-api-platform.md` 完整预览，不修改该规格原文，不扩展宿主 API。

## 复现与根因

原文 39,174 字节，解析成功；实际派生树包含 1,181 个原生节点、898 个富文本标记额度和 4 个链接目标，正文合计 2,083，超过公开 UI 的 2,048 额度，尚未计入根容器和工具栏。插件因而显示“此文档超出原生预览限制。”，不是宿主 1 MiB 源文件限制。

先新增实际原文回归：未修复时访客测试返回 `UI node quota exceeded`；既有正式 ZIP → Manager → 原生窗口测试也显示 `preview-limit`。对照节点、富文本与链接消耗后，定位到单段列表条目的冗余 `item-body` Column。

## 修改

- 单子节点的列表正文直接进入列表行，保留正文自身的 RichText、源码范围与 grow 布局；多个段落、嵌套列表仍使用带 6 px 间隔的 Column。
- 保留每个表格单元格独立的富文本节点、任务框、图片、链接与主题。没有截断正文，也没有提高解析或通用 UI 配额。
- 清单、贡献和 Cargo 版本统一提升为 0.17.0；继续使用现有公开能力与宿主 SDK。

## 验证

- `target/release/editor-app.exe --plugin-cargo plugins/markdown/Cargo.toml test`：73 passed、0 ignored，包括原文完整场景校验、末尾源码映射以及紧凑列表的多段落、嵌套、任务、富文本和链接回归。
- `scripts/build-plugins.ps1 -Packages markdown -HostExe target/release/editor-app.exe`：独立 WASM 构建和正式 ZIP 打包通过。
- `cargo test -p editor-app delivered_markdown_platform_specification_previews_and_scrolls_to_end -- --ignored --nocapture`：1 passed。实际 ZIP 安装、完整场景校验、预览可滚动至原文最后一项，延迟同步后末尾仍在视口且位置稳定。
- 原生测试二进制的 `markdown_tests::small_scroll --ignored --nocapture`：3 passed，覆盖微小滚动、长表格与输入在途的滚动所有权。
- `cargo test --workspace --exclude editor-app`：136 passed、126 ignored；`cargo check --workspace`：通过。常规 workspace 测试跳过的 WASM 验收不计入通过。
- `cargo fmt --check` 与独立 Markdown `cargo fmt --manifest-path plugins/markdown/Cargo.toml --check`：通过。

末尾测试曾错误地假定富文本范围包含终止换行，已改为检查宿主内存快照最后一个非空白字节。超大滚轮输入会超过内容边界，原生重测后按新高度夹紧；测试检查夹紧后的位置稳定及最后一项可见，不将合法夹紧误判为回顶。

## 限制与失败归因

任务框相邻套件为 1 passed、3 failed；临时恢复本次优化前的 Column 布局，重新构建真实 WASM 对照包并运行同一原生测试二进制，仍为相同的 1 passed、3 failed（`task_checkboxes.rs:53`、`:200` 与 `harness.rs:170`）。正文写入后的旧节点 ID 断言、第二次键盘切换、后续控件定位均不是此次容器优化引入，最终已恢复修复代码和正式包。这组失败不计入通过。全仓库 `git diff --check` 检出了既有 `README.md:8` 行尾空白，本次不改动无关文件。

原生验收由 GPUI 实际控件与正式包自动化驱动，未做人工窗口操作。更大的派生场景仍受原有有限配额约束，本次不宣称任意长度 Markdown 均可完整预览，也不宣称原输入性能问题全部通过人工验收。

## 交付

`dist/plugins/markdown.zip` 与 `dist/editor/plugins/markdown.zip` 均为 0.17.0，已同步配套 `bundle-defaults.json`。两个 ZIP 的 SHA-256 均为 `464A64731A1F6066FEBBC7A77A9161A2C36A0880473E919847EE7F9716B283A9`。最终 ZIP 的 WASM 与原生测试通过的修复包逐字节哈希一致：`E4136369223B4DFF414FCFBDB56EEFD79CE7CD00B0A13FF15FA0AF5734B06F0C`。

使用插件管理器更新该包；发行目录文件同步不等于现有已安装插件自动更新。本次未提交或推送。任务文件范围内 `git diff --check` 通过，仓库整体的既有空白问题如上记录。
