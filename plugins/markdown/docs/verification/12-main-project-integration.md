# 12 — 主项目宿主集成与发行包验证

日期：2026-10-04。状态：主目录代码集成、配套重建、专项原生验收及最终工作区门禁已完成。

## 实际基线与范围

用户要求“全部补齐回主项目”。原目录 `C:/Projects/RustProjects/Editor` 的 `main` 从 `3d6d21cb` 快进到本次合并提交 `e0f69d3b51032a89812db47a28fea3ec7c0272fa`；宿主实现已经进入原项目，不依赖从另一个工作树启动程序。合并保留主项目的运行日志、确认提醒和错误恢复功能，并接入完整 Markdown 公开能力。

Markdown 版本为 0.11.1，清单、Cargo 与贡献文件一致。源码 grammar、十三个常用格式命令、原生预览、三种布局、内容块双向滚动、图片预览和同级 img 序列导入、任务框、链接与代码块高亮均使用公开接口。SVG 0.3.0 声明同样的三态及三个独立包图标，取代原单个 SVG 底栏入口。

合并时额外修复插件服务委托导航的来源权限：消费者不能借用提供者权限打开相对文档或外部 URL。主窗口恢复失败在原有日志状态弹层展示；查看后确认当前错误，再次发生相同错误仍会重新提醒，详细原因继续可查看。

主目录发行核对还发现，合并时采用已有 PowerShell 5.1 打包改动遗漏了历史分支中的 `bundle-defaults.json` 生成及可选资源目录检查。已恢复，保留原有路径兼容；索引初次创建使用两参数 Move，更新使用 Replace 与 NullString，兼容 .NET Framework 并保持原子写入。新增实际打包输出回归先精确失败于缺少索引，再通过 PS 5.1 初建与同目录二次替换。日志为主目录 `target/markdown-main-catalog-red.log`、`markdown-main-catalog-green.log`、`markdown-main-catalog-replace-green.log`。

## 保留已有工作

原主目录 214 个已改动／未跟踪路径在集成前逐个保存内容及 SHA-256。恢复后除五个明确合并重叠文件（Cargo.toml、Cargo.lock、editor-app/Cargo.toml、README.md、build-plugins.ps1）外全部与快照逐字节一致；其中根 Cargo.toml 已包含相同插件排除配置。随后只同步本任务相关版本和文档。用户的日期显示、故障恢复、资料重归档等改动仍留在工作区，未把它们当作本任务新成果或提交。

备份位于原 Markdown 工作树 `target/main-integration-backup/`；临时保存 stash 保留可恢复。主项目无冲突项，未 reset、清空用户数据、强推或改写历史。本次没有推送 main、发布 Release 或更改父方案议题。

## 集成提交前的实测

下列日志位于 `C:/Users/Tmiracle/.codex/worktrees/markdown-plugin/Editor/target/`，使用与合回 main 相同的集成源码；不冒充原主目录重建结果。

| 命令／场景 | 实际结果 |
| --- | --- |
| `cargo fmt --check` | 通过，main-integration-fmt.log |
| `cargo test --workspace --exclude editor-app` | 104 passed、112 ignored，main-integration-workspace.log；ignored 未执行 |
| `cargo check --workspace` | 通过，main-integration-check.log |
| `service_navigation --ignored --test-threads=1` | 真实独立 SDK ZIP 经公开 Manager：2 passed、0 ignored；越权请求拒绝、明确授权请求受理 |
| 原生 SVG 模式、Markdown 三态、十三个空选区模板及两个选区工具栏场景 | 5 passed、0 ignored，真实 ZIP／Base 控件／布局／Undo 与主题检查 |
| 首次权限确认、拒绝未声明原生 grammar 注入、安装准备失败、两次相同恢复错误 | 4 passed、0 ignored，真实生产 worker 和原生状态弹层 |
| 现有状态弹层测试 | 10 passed、0 ignored，main-integration-status_tests-green.log |

以上原生场景合计 19 项，均显式运行；不包含历史 0.11.0 的完整 62 项。

恢复错误测试第一轮失败于原生确认行为，修复后又发现测试使用了不存在的关闭选择器；改用真实 Escape 关闭弹层后最终通过。编译缓存曾被另一目录的构建污染，定向清理协议／平台 crate 后检查通过；主目录重建使用自己的 target，不复用该工作树 target。

## 主目录重新构建

所有后续命令在 `C:/Projects/RustProjects/Editor` 中执行，清除 CARGO_TARGET_DIR，设置 CARGO_BUILD_JOBS=1、RUST_MIN_STACK=33554432。实际完成结果和最终文件摘要如下。

已完成：`cargo fmt --check` 通过；`cargo test --workspace --exclude editor-app` 为 104 passed、0 failed、112 ignored。首次 `cargo check --workspace` 读到旧版协议／格式缓存，54 条缺失 API 错误与已存在的源码声明不符；执行 `cargo clean -p plugin-protocol -p plugin-schema -p platform-windows` 后再检查通过（6.88 s、保留三条既有警告）。日志分别为 `target/markdown-main-fmt.log`、`markdown-main-workspace.log`、`markdown-main-check.log`、`markdown-main-targeted-cache-clean.log`、`markdown-main-check-green.log`。只清理三个 crate 的构建缓存，没有清空源码、用户插件或整个 target。

`cargo build -p editor-app` 通过，6m58s；仓库外 `./scripts/verify-plugin-sdk.ps1` 通过，57.74s，实际导出、损坏后修复、独立 WASM 编译并将同一 ZIP 提供给 SDK/runtime 回归。Markdown 独立 `cargo fmt --manifest-path plugins/markdown/Cargo.toml --check` 通过；正式主目录 EXE `--plugin-cargo plugins/markdown/Cargo.toml test --lib` 为 64 passed、0 failed、0 ignored。日志为 `markdown-main-build-debug.log`、`markdown-main-sdk.log`、`markdown-main-guest-fmt.log`、`markdown-main-markdown-guest.log`。

`./scripts/package-editor.ps1` 通过，232.82s，输出原项目 `dist/editor/editor-app.exe` 和八包；`powershell.exe -NoProfile -File ./scripts/verify-plugin-packaging.ps1 -DefaultPackages -HostExe ./dist/editor/editor-app.exe` 通过，18.45s，实际 PowerShell 5.1.26100.9444 全包路径、非空 Markdown 索引、ZIP 哈希及两个扩展名均通过。正式包原样复制到 `dist/plugins/`，原生测试消费相同字节。日志为 `markdown-main-release-package.log`、`markdown-main-packaging-ps51.log`。

## 正式产物绑定

主目录 `target/markdown-main-artifact-audit.log`、`markdown-main-artifacts.json` 逐包核对 protocol 7、无源码／缓存泄漏、测试与正式 ZIP 哈希一致；Markdown 15 个条目全部与当前主目录源码或实际编译组件 SHA-256 相同，贡献版本一致且包含 12 项必需能力。SVG 0.3.0 包包含四个原缩放图标和三个模式图标。正式 EXE 与本目录 `target/release/editor-app.exe` 完全一致。

- 最终刷新后的 `dist/editor/editor-app.exe` SHA-256：`d560f59b4f724947868fa8c13c69641c32d67b0b3038242cbf3f0dc9ecc0eb64`。首次配套发行 EXE 为 `d98186563dcbddae9b723753522f64a9e42f65ef2ef18e729eeb3cfea54048df`；下文单列后续主目录宿主刷新。
- `dist/editor/plugins/markdown.zip`（0.11.1）SHA-256：`7191b56eb8f7fd2a79a5fc560a18f625d3b041142bf83b421f5a8cbfa4dcc9d2`。
- `dist/editor/plugins/svg.zip`（0.3.0）SHA-256：`db96b735020f15a39cfd9be33dfb0d2ec9fd76fb085b4643264e4f17b8db9d28`。

实际 `dist/editor/plugins/bundle-defaults.json` 只有一条 Markdown 首次提供记录，版本 1、md／markdown 扩展名、SHA-256 与以上实际 ZIP 一致。源码合回 main 及构建产物均已完成；下面单列本次主目录原生回归。

## 主目录宿主与真实包回归

`cargo test -p editor-app -- --test-threads=1`：246 passed、0 failed、98 ignored，执行 121.36s；日志 `target/markdown-main-editor-default.log`。该组包含合并后恢复错误确认、十项状态弹层、恶意 grammar 注入，以及原主目录既有日期／恢复相关改动；ignored 单列，未计通过。

新主目录 SDK 构建的同一独立 ZIP 经公开 Manager 的 `sdk_distribution --ignored --test-threads=1` 为 1 passed、0 ignored（18.14s）；`service_navigation --ignored --test-threads=1` 为 2 passed、0 ignored（73.29s）。日志 `markdown-main-runtime-sdk.log`、`markdown-main-service-navigation.log`。没有仅复制历史 SDK fixture 后假称独立验证。

正式 ZIP 的下列原生组全部显式执行 `cargo test -p editor-app <过滤词> -- --ignored --test-threads=1`，共 16 passed、0 failed、0 ignored。所有日志位于本目录 `target/`，前缀为 `markdown-main-`。

| 过滤词／场景 | 结果与日志后缀 |
| --- | --- |
| `extensions::markdown_tests::format_toolbar` | 3 passed（93.27s），extensions-markdown_tests-format_toolbar.log；真实十三按钮、中文选区、本地化空模板及 Undo／Redo |
| `extensions::markdown_tests::modes` | 2 passed（47.41s），extensions-markdown_tests-modes.log；Markdown 与 SVG 实际三按钮、分隔线、布局、文档／撤销保留及主题 |
| `delivered_markdown_combines_edits_images_tasks_links_ime_and_scaled_layout` | 1 passed（45.94s），同过滤词 .log；格式、图片、任务、链接、marked composition 与缩放组合 |
| `delivered_markdown_code_uses_an_independent_enabled_language_provider` | 1 passed（24.07s），同过滤词 .log；独立已启用 WASM 提供者实际代码颜色 |
| `extensions::markdown_tests::synchronized_scroll` | 6 passed（177.63s），extensions-markdown_tests-synchronized_scroll.log；双向内容块、开关记忆、图片／表格／窗口重排、旧结果与关闭撤销 |
| `delivered_markdown_image_drop_imports_multiple_formats_and_previews_saved_files` | 1 passed（23.20s），同过滤词 .log；实际编码、多图 img 同级序列及真实预览 |
| `delivered_markdown_preview_tracks_unsaved_native_edits_and_reclaims_split` | 1 passed（17.56s），同过滤词 .log；未保存实时预览与撤销布局 |
| `fresh_markdown_first_use_confirms_real_package_and_retains_disabled_choice` | 1 passed（30.01s），同过滤词 .log；真实 Base 权限确认、新 ZIP 热生效及禁用选择保留 |

`cargo run -p editor-app --example svg_preview_render -- dist/editor/plugins/svg.zip plugins/svg/examples/gear.svg target/markdown-main-svg.png` 通过（完整阶段 38.31s）。实际 SVG 0.3.0 WASM 和同一原生 raster 的颜色、透明洞、半透明及棋盘层断言通过；已查看输出 PNG，几何与透明背景正常。日志为 `target/markdown-main-svg-render.log`。

验收期间主目录另一任务继续新增 host execution 服务实现。保留这些独立改动，未将其列为本任务成果；本轮 Markdown／SVG 验收源码和正式资源明确绑定，工作区门禁在最后再执行一次，不依赖早期缓存结果。

收尾时另一任务已在 main 提交 `4d4a7b6`；本次 `e0f69d3` 仍为其祖先，Markdown 集成没有被撤销。后续配套宿主刷新保留该提交及目录里继续进行的改动，独立运行／调试功能不作为本任务已验收的产品能力。

最终门禁：`cargo test --workspace --exclude editor-app` 为 107 passed、0 failed、113 ignored；`cargo check --workspace` 通过（4.97s）。首轮最终 `cargo fmt --check` 仅报告并行任务新增的 `manager/host_services.rs` 与 `tests/host_execution.rs` 格式差异；这些文件随后由其原任务更新，再执行 `cargo fmt --check` 通过。未擅自格式化或改写该任务文件。实际日志为 `target/markdown-main-final-workspace.log`、`markdown-main-final-check.log`、`markdown-main-final-fmt.log`、`markdown-main-final-fmt-recheck.log`，最终 `markdown-main-final-gates.json` 的三项退出码均为 0。

验收结束后再次运行正式产物审计：八包、Markdown15资源、SVG七图标、测试复制品、EXE 与索引 SHA-256 仍全部一致，未在验收途中替换正式 ZIP；证据为 `target/markdown-main-artifact-audit-final.log`。相关文档相对链接、路径和 `git diff --check` 收尾通过。

### 最终宿主刷新与再次验收

在另一任务合入后，以原主目录当前 main 再构建 Debug 与 Release（日志 `markdown-main-head-debug.log`、`markdown-main-head-release.log`），分别通过，37.04s／3m11s；Release 原样复制到 `dist/editor/editor-app.exe`。插件 ZIP 与索引保持上述经过完整专项组的相同字节，未重新打包或改变权限。

并行源码写入时，首次 workspace 检查准确失败于 `editor-core/src/run.rs` 已导入但 store 尚未定义的 `storage_path`；原任务写入实现后，`cargo check --workspace` 重跑通过（1.95s）。未替原任务添加实现或撤销其改动。当前主目录非 UI workspace 为 114 passed、0 failed、113 ignored，日志 `markdown-main-head-workspace.log`；最新 `cargo fmt --check` 通过，日志 `markdown-main-head-fmt-final.log`。

刷新宿主后再次执行相同真实 ZIP 的模式组（2 passed、0 ignored，52.28s）和首次安装组（1 passed、0 ignored，32.20s），日志 `markdown-main-head-modes.log`、`markdown-main-head-first-use.log`。这是上述 16 项中的三项重复复核，未虚增独立用例总数。最后的产物审计记录 main 提交 `4d4a7b6d90130cf6e687c46c610c5ca38f1f89c1`，八包资源／测试副本／索引仍一致，最终 EXE 与原目录最新 release EXE 同哈希 `d560f59b…c0eb64`。

### Standards

独立只读代理对集成、最终恢复错误 GREEN、十项状态弹层以及打包脚本 PS5.1 修正签收：硬规范 0 项遗留，Fowler 0 项。原规范发现及测试夹具失败均已修正并实测；未运行 Cargo、Git 写操作或修改文件。

### Spec

独立只读代理签收恢复错误确认及完整正式发行／非空索引／MD15资源与SVG七图标：0 项遗留问题。随后主代理执行以上 16 项原生组全部通过；用户私有注册表升级仍未操作、不计完成。

## 使用与限制

安装状态属于用户私有目录，源码合并、cargo clean 和生成新 ZIP 都不会自动覆盖既有安装或卸载选择。本次不直接编辑 registry 或权限授权；请从原项目的新 EXE 启动，在插件管理中安装／更新 Markdown 0.11.1 与 SVG 0.3.0，按正常确认窗口授予清单权限。若已明确卸载 Markdown，首次提供机制尊重该选择，需要主动安装。

原生回归在 Windows GPUI 测试环境实际操作控件、输入与布局，不能宣称已自动更新用户正在使用的安装记录。历史完整 M01–M16 结果保留在[11](11-distribution-acceptance.md)；本轮单列新增集成和重建结果。macOS／Linux 未实测。
