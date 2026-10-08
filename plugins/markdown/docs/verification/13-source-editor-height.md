# 源码编辑区只有一行的布局修复

日期：2026-10-04。主目录：`C:/Projects/RustProjects/Editor`。代码基线：`main` 的 `4d4a7b6d90130cf6e687c46c610c5ca38f1f89c1` 及保留的既有工作区改动；本次仅补齐通用源码高度布局与回归。状态：专项原生回归、仓库门禁及 Debug／Release 宿主重建完成。

## 问题与复现

用户实际窗口已有 Markdown 工具栏、分栏预览和底栏模式按钮，但左侧源码只显示第一行，下方留下大片空白。

沿既有公开接缝验证：正式 Markdown 0.11.1 ZIP → `Manager.install` → 文档打开 → GPUI 原生窗口与 Base 编辑器。新增长文档回归直接读取 Base 公共 `input_bounds` 与可见行范围，而非只检查分栏外框存在。

首次 RED：1400 × 900 窗口中，源码输入区高度为 21 px，工具栏下方至编辑区域底部仍有 728 px；源码容器仅 114 px 高，可见行范围为 `0..3`。日志：`target/markdown-height-red.log`；1 failed、0 ignored。

## 原因与修复

通用 `render_source_viewport_probe` 使用占满外框的普通 block 容器，但其源码子元素依赖 `flex_1` 获得剩余高度。观察容器没有建立纵向 flex 布局，源码高度退化为内容的最小高度；内部百分比高度也就只得到一行输入区。

在 `crates/editor-app/src/extensions/preview/viewport.rs` 为观察容器补上 `flex` 与 `flex_col`，附带解释高度传递的注释。既有编辑器和工具栏自行分配纵向空间，未新增插件 ID／语言专属分支、第二份文本状态或新的公开 API。

先单独验证高度传递这一假设，再使用同一回归得到 GREEN：1 passed、0 ignored，日志 `target/markdown-height-probe-green.log`。无需修改工具栏的高度或编辑器自身实现。

## 原生回归

新回归位于 `crates/editor-app/src/extensions/markdown_tests/source_layout.rs`。逐一比较测试 ZIP 与正式发行 ZIP：Markdown 15 个条目、SVG 10 个条目的 SHA-256 均一致，包含清单、真实 WASM 与全部资源；重新打包后的 ZIP 容器摘要不同，未将其声称为同一容器字节。

`cargo test -p editor-app extensions::markdown_tests::source_layout -- --ignored --test-threads=1`：2 passed、0 failed、0 ignored，72.76 s；日志 `target/markdown-height-layout-green.log`。

- 160 行中文文档，1400 × 900、950 × 700、700 × 550、2100 × 1160 四种窗口尺寸及深浅主题；分栏／仅编辑两态，实际输入高度填满工具栏下方空间，可见多行。
- 编辑器下方真实鼠标点击选中后续行；真实滚轮改变可见源码行范围，文档内容保持原样。
- 空 Markdown 在两态下仍有完整输入区域；SVG 0.3.0 无格式工具栏也占满源码区域；切换普通文本后无遗留工具栏或预览。

后续显式执行 `cargo test -p editor-app <过滤词> -- --ignored --test-threads=1`：

| 过滤词 | 结果 | 日志 |
| --- | --- | --- |
| `extensions::markdown_tests::synchronized_scroll` | 6 passed、0 failed、0 ignored，208.86 s | `target/markdown-height-sync.log` |
| `extensions::markdown_tests::modes` | 2 passed、0 failed、0 ignored，47.79 s | `target/markdown-height-modes.log` |
| `extensions::markdown_tests::format_toolbar` | 3 passed、0 failed、0 ignored，109.95 s | `target/markdown-height-toolbar.log` |

连同新增布局组，本次专项原生回归为 13 passed、0 failed、0 ignored。覆盖正确源视口尺寸下的双向块定位、图片／表格／分隔线重排、独立公开 SDK 提供者、模式记忆、格式模板、中文选区与 Undo。未复用历史完整组的通过数。

## 仓库门禁与宿主构建

所有命令在原主目录执行，清除 `CARGO_TARGET_DIR`，设置 `CARGO_BUILD_JOBS=1`、`RUST_MIN_STACK=33554432`，未复用另一工作树的构建目录。

- `cargo test -p editor-app -- --test-threads=1`：246 passed、0 failed、100 ignored，125.51 s；日志 `target/markdown-height-native-default.log`。未把默认跳过的包测试计为通过，上述 13 项已另行显式运行。
- `cargo test --workspace --exclude editor-app`：114 passed、0 failed、113 ignored，逐 crate 结果见 `target/markdown-height-workspace.log`；完整执行 136.31 s。
- `cargo check --workspace`：通过，2.44 s；日志 `target/markdown-height-check.log`。
- `cargo fmt --check`：通过，1.28 s；日志 `target/markdown-height-fmt.log`。最初发现本次新测试的格式差异，已仅修正该文件后重跑。
- `cargo build -p editor-app`：通过，39.40 s；日志 `target/markdown-height-debug.log`。

结构化退出码记录：`target/markdown-height-results.json`；以上退出码均为 0。保留既有未使用成员与 Tree-sitter／Wasmtime 链接警告；未修改并行运行／调试任务的代码，也未替其宣称产品验收完成。

## 配套程序

`cargo build -p editor-app --release` 通过，106.40 s，日志 `target/markdown-height-release.log`。原样复制到 `dist/editor/editor-app.exe`，与本目录 `target/release/editor-app.exe` 的 SHA-256 一致：`4ef844d89ba6438d5371f4aa8a90e2c6d50f14cac54ad73136115cef6e3e820d`。结构化记录为 `target/markdown-height-release-result.json`。

发现旧 Release 程序仍在运行，先将这一份构建输出重命名保留为 `target/release/editor-app.before-height-20261004-205458.exe`，再生成原路径的新程序；操作前核对源／目标均位于主目录 `target/release`。没有终止旧进程或丢弃未保存文档。

这是宿主布局修复，Markdown 0.11.1 与 SVG 0.3.0 ZIP 的协议、权限与内容无需更新，正式 ZIP 与首次提供索引保持不变。关闭当前旧窗口后启动 `dist/editor/editor-app.exe`，或正常启动新生成的 `target/release/editor-app.exe` 即可生效；无需重装插件或执行 `cargo clean`。

原生交互由 Windows GPUI 测试窗口完成，没有自动重启用户的现有窗口，也没有编辑私有安装记录。macOS／Linux 未实测。
