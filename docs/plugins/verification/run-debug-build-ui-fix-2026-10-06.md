# 运行配置与顶栏 UI 修复验收（2026-10-06）

状态：用户指出的两处 UI 偏差已修复，并完成回归、真实包检查及 Windows 实际窗口核对。修改仅在 `Editor-run-debug-build` 分支与工作区，未合并主分支。

## 依据与先前记录更正

依据为父设计 [#48](https://github.com/T-miracle/Editor/issues/48) 和已确认的 [B1 原型](../specs/assets/run-config-b1.png)。原型图片从主工作区已有文件取用，在本分支归档；没有修改主工作区。

[先前终验](run-debug-build-completion-2026-10-06.md) 的 R01 验证了功能入口、页签和保存行为，未充分验证原型外观。用户截图确认顶栏仍使用文字按钮，表单的保存栏位于内容中间且下方留有大片空白。因此不能沿用此前记录宣称 B1 视觉验收完整，本记录补充并替代 R01 的视觉结论。

## 可复现失败与修复

修改前新增的两条原生布局回归均失败：

- `b1_titlebar_actions_are_compact_icons`：构建按钮约为 `44×24`，不满足紧凑图标按钮的几何要求。
- `b1_modal_keeps_selector_tabs_and_footer_in_their_layout_regions`：表单高度 `582`，保存按钮下方仍有约 `250` 像素空白；顶部只有保存数量，页签未铺满。

失败原因是布局与控件选择：表单把内容和底栏放在同一顺序容器，顶部配置入口、页签与顶栏操作使用了文字占位。修复复用本地 `ui/` 控件与 gpui-base 行为，没有新增宿主专属业务或复制上游控件实现。

- 顶栏为配置／会话下拉、锤子、绿色运行三角、调试虫和停止方块，并保留分隔线、禁用原因及中英文无障碍名称。重新运行和立即终止移入统一菜单；普通执行停止中时原停止位置提供立即终止。
- 弹窗按 B1 分为配置选择与来源区、等宽四页签、可滚动字段区、状态行和固定底栏。保存位置改为下拉，取消为普通按钮，保存为主按钮；关闭入口使用图标。
- 选择既有配置同时替换草稿与保留的编辑状态，页签切换不重建输入。下拉属于当前表单，关闭或替换草稿会撤销它。

下拉键盘用例另外发现：回车选择后，外层 Dialog 的 Confirm 也被触发。已在本地 PopupMenu 消费 Confirm／Cancel 动作，阻止回车连带保存或 Esc 连带关闭表单。针对该行为的原生用例先失败、修复后通过。

生产编译曾发现新渲染器误用仅测试构建可见的 `field_input`；改为读取原表单的输入状态集合后，重新运行生产编译和完整应用测试。没有扩大测试辅助接口的可见性。

## 候选与实际检查

顶栏独立候选 tree 为 `4a475df9642a30c5dcd4ba8d23f8799e5ad8c88d`，提交为 `d5bffa265aaa100a0d0767de12e1fc67ddc41ec0`。弹窗及最终生产源码候选 tree 为 `8505c5f90a067506cdcccd0c903e965492e30ef7`。两者从 Git index 导出到仓库外临时目录构建，未借用未暂存生产源码。最终新增的维护者记录与索引不参与编译，生产源码保持该候选内容。

候选先用 Git ZIP 和显式 UTF-8 解包。最初使用 Windows tar 解包中文路径失败，未以其不完整目录作为交付验证依据。所有 Cargo 检查在隔离候选运行，复用本分支 `target/` 缓存；保留并排除本轮开始前已有的 `Cargo.lock` 四行改动，其工作副本 blob 仍为 `17d6f7ba2dc56b0a919410431e1871430186f097`。

下表 Cargo 命令均带 `--target-dir C:/Projects/RustProjects/Editor-run-debug-build/target`；最终构建与真实包用例另带 `--locked`。应用测试使用进程级 `RUST_MIN_STACK=16777216` 及 `--test-threads=1`，不改全局环境。日志在 `target/run-debug-build-ui-fix/`。

| 检查 | 实际结果 | 日志 |
| --- | --- | --- |
| 两个独立候选 `cargo fmt --check` | 通过 | 终端输出，无格式错误 |
| 顶栏候选非 UI workspace 测试、workspace 编译 | 通过 | `title-exact-non-ui.log`、`title-exact-check.log` |
| 顶栏候选 `cargo test -p editor-app run_ui_tests`、`exposes_run_title_icons` | 19 + 1 passed，0 failed | `title-exact-run-ui.log`、`title-exact-assets.log` |
| 最终候选 `cargo test --workspace --exclude editor-app` | 175 passed、0 failed、160 ignored | `form-final-non-ui.log` |
| 最终候选 `cargo check --workspace`、`cargo build -p editor-app` | 通过 | `form-final-check.log`、`form-final-build.log` |
| 最终候选 `cargo test -p editor-app` | 361 passed、0 failed、118 ignored；包含 23 项 Run UI 用例 | `form-final-app-all.log` |
| `cargo test -p editor-app native_discovery_tests -- --ignored` | 3 passed、0 failed、0 ignored | `form-final-native-discovery.log` |
| `cargo test -p editor-app native_shared_configuration -- --ignored` | 1 passed、0 failed、0 ignored | `form-final-native-sharing.log` |
| 顶栏候选 `website/` 中 `npm run build`、`npm test` | 构建成功；17 passed、0 failed、0 skipped | `title-site-build.log`、`title-site.log` |

站点子树 `a802c0a6741862843b9d596bdd2a5e41f2d64238` 在两个候选间一致，复用该站点检查。默认测试中的 ignored 项未计为通过；本轮显式执行的四项真实包用例使用上轮终验已独立构建、且本轮源码未改变的包，复制到当前候选并核对 SHA-256。没有重新宣称本批所有历史真实包测试已重跑。

| 夹具 | SHA-256 |
| --- | --- |
| `terminal.zip`（0.11.0） | `1dc0634d3b38bf630888067fda10943b69cbb9a4c1bfc186361fa0cce48c250b` |
| `rust.zip`（0.4.0） | `4300e070832a174c6835b0517fb889e6ba36bdb91aa1ed31599aa7497c28bf03` |
| `run-target-example.zip` | `fbb9802a9b100b136363bac68fabfdab0d9477ab073a46039ea1c23d85b0dfbc` |
| `rust-debugger.zip`（0.1.0） | `e006b0831411f65d48479c1071d4d2fec80beab44a732c5b67d820f51c3437fa` |
| 已核验的 CodeLLDB 1.12.3 VSIX | `a916e509308dac817732f63ca604a8b93ed29cd16f38a2fa9f0b64ed58e8f51a` |

## Windows 实际窗口

正常关闭旧分支程序后启动最终候选编译的 `target/debug/editor-app.exe`，参数指向本分支工作区。实际检查包括：

- 顶栏四个图标实际绘制，统一菜单包含重新运行与终止。
- 弹窗配置选择区、来源行、等宽下划线页签、左右字段和固定底部保存栏可见；底栏下方不再留有大片空白。
- 名称框 Microsoft Pinyin 输入 `n → ni`，候选出现在名称字段旁，空格提交“你”并关闭候选；配置选择器同步显示该名称。
- 点击构建页后以 Right 切换调试和环境页，Home 回基本页，中文名称保持。
- 保存位置下拉的 Esc 只关闭菜单；Down／Return 选择“项目共享”，表单仍在、草稿仍在，未保存或启动程序。
- 最后取消验收草稿，未创建用户配置或共享文件。窄窗口、两种主题与语言、字号 24 下底栏可达性由新增原生几何用例覆盖；不将这些自动化结果写作本轮真机主题切换。

另一个实际环境差异：本机插件管理页仍显示已安装终端 0.7.1、Rust 0.2.1；调试页显示终端执行契约不匹配。上表真实流程使用本分支的新包，不能当作已更新用户的安装。编译 EXE 不会更新已安装的 WASM 包，本轮没有静默升级插件；体验本批运行与调试能力仍需按正常安装／更新流程使用新包。

本记录仅修正 UI 和补充当前窗口观察，不改变父设计 #48，也不重写实施工单的判据。
