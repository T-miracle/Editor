# B3 双栏简洁配置实现与验证（2026-10-06）

状态：原生实现、自动化 UI 回归及下列四项真实插件用例已通过；新版窗口的人工视觉核对和系统输入法候选窗口尚未重新验收。本记录不把设计原型图片或 B1 的人工检查作为 B3 真机证据。

## 范围与依据

用户确认采用 [B3 双栏简洁版](../specs/run-config-simple-design.md)，并要求默认运行命令／目标由插件提供。实现复用已有目标发现、提供者绑定、准备动作及会话契约，不增加宿主语言分支、不扩展 WIT，不改变父议题或实施工单判据。

所有修改与命令均在 `C:/Projects/RustProjects/Editor-run-debug-build`、分支 `Editor-run-debug-build`。起始 HEAD 为 `4fe4cd9e435c4eb8485f86eef21be24774d653c0`，本轮代码与文档仍是工作区改动，未提交、推送或合并到主工作区 `Editor`。开始前已有 B2/B3 设计文件、索引改动与 `Cargo.lock` 尾部四行；没有将它们全部当作本轮实现成果。

## 行为与实现

- 主弹窗默认约 `940×502`：左侧配置列表，右侧名称、目标和可选参数；启动前与更多设置默认收起。窄窗口改为上下布局，编辑区滚动，保存栏固定。Shell 配置直接显示必填脚本正文。
- 加号选择插件目标或手动程序／脚本；复制和删除位于更多菜单。复制先生成草稿，删除单独确认并立即持久化；外层取消不撤销已明确确认的删除。
- 参数、环境变量、工具路径和断点按需进入同一原生窗口内的详情编辑面板。取消详情恢复原值；切换已修改配置须选择保存、放弃或取消。保存隐藏非法字段时会展开或打开相应编辑器。
- 构建动作与运行前步骤保持独立，使用程序、Shell 或构建引用的结构化编辑器。构建引用沿用现有配置名称契约，不自行改为 ID。参数保留字面空格、引号与标点，不拼接 Shell 命令。
- 本地控件继续使用 gpui-base 行为；折叠控件、按钮、弹窗和项目图标样式在 `ui/` 层实现，中英文本同步维护。表单状态从原 `run/ui.rs` 移至 `run/ui/form.rs`；纯搬迁后先通过原有 23 项 UI 用例，再改布局。

插件默认目标的流程为：

1. 可信工作区首次打开配置窗口时查询已启用插件，之后可显式再次发现；发现只更新候选目录。
2. 选择候选调用 `RunControls::target_template`，带入插件名称、程序或不透明绑定、默认参数及构建动作，不写配置、不选择运行会话、不启动进程。
3. 保存才创建配置。提供者绑定不作为可编辑程序路径；必需的准备动作锁定，不能删除或移动普通步骤越过它。即使提供者显示标签含首尾空格，保存仍保留绑定。
4. 再次选择已保存目标复用用户配置，不用新默认值覆盖用户修改；运行时沿用提供者准备与实际产物解析。顶栏原有的显式确认目标入口仍可直接创建配置。

插件没有提供目标时可手动配置。工作目录默认沿用工作区，环境变量和本机工具路径由用户设置；当前目标契约没有被扩展为提供这些默认值。

## 回归与修复依据

原生交互测试发现并修复了以下问题：

- 展开内容受容器的自动最小高度影响，撑大弹窗并裁切底部保存栏；在本地 DialogContent 允许内容缩入滚动区。
- 父 Dialog 的回车 Confirm 抢先处理按钮／类型标签的原生激活；本地控件键上下文保留 GPUI 的键盘点击，不将回车连带解释成整体保存。
- 原选择候选路径先持久化再打开表单，取消无法撤销新配置；改为无副作用模板，保留顶栏的显式确认语义。
- 提供者标签中的首尾空格被当作路径变化，可能丢失准备绑定；增加实际原生保存回归，保留未变的完整标签与不透明绑定。

新增七项交互用例覆盖默认目标的选择／取消／保存、详情取消与多行输入、三种切换决定、隐藏字段错误、程序动作参数、Shell 与构建引用、复制与删除边界。既有用例同步检查默认折叠、双栏、固定底栏、两种主题与语言、`520×420` 窗口、字号 24、焦点、键盘和滚轮。完整应用测试还覆盖新增图标资源的实际解析。

## 实际命令与结果

Cargo 命令在分支工作区执行。应用测试设置进程级 `RUST_MIN_STACK=16777216`，使用 `--test-threads=1`；未修改全局环境。日志位于该工作区的 `target/run-config-design/`，为本地构建产物，不随仓库发布。

| 检查 | 结果 | 日志 |
| --- | --- | --- |
| `cargo fmt --check` | 通过 | 终端输出 |
| `cargo test --workspace --exclude editor-app` | 175 passed、0 failed、160 ignored | `b3-non-ui.log` |
| `cargo check --workspace` | 通过 | `b3-check.log` |
| `cargo test -p editor-app run::run_ui_tests -- --test-threads=1` | 30 passed、0 failed、0 ignored | `b3-ui.log` |
| `cargo test -p editor-app -- --test-threads=1` | 369 passed、0 failed、118 ignored；含上述 30 项 UI 用例与最终标签空格回归 | `b3-app-all.log` |
| `cargo test -p editor-app native_discovery_tests -- --ignored --test-threads=1` | 3 passed、0 failed、0 ignored | `b3-native-plugin-targets.log` |
| `cargo test -p editor-app native_shared_configuration -- --ignored --test-threads=1` | 1 passed、0 failed、0 ignored | `b3-native-sharing.log` |
| `cargo build -p editor-app` | 通过，生成 `target/debug/editor-app.exe` | `b3-build.log` |
| `website/` 中 `node --test tests/*.test.mjs`（`npm test` 的实际脚本） | 17 passed、0 failed、0 skipped | `b3-website-test.log` |
| `git diff --check` 与文档路径检查 | 通过 | 终端输出 |

完整应用测试是在标签空格修复后执行；之后仅有 rustfmt 与注释调整，并重新完成格式检查及生产构建。默认测试中 ignored 项不计为通过；下列真实流程是本轮显式执行的四项，不宣称其余历史 ignored 测试均已重跑。

- 独立声明式目标插件：发现、原生选择草稿、编辑并保存、构建、运行及显式修复目标；证明宿主不依赖 Rust 插件专属分支。
- Rust 插件：实际项目发现，原生配置界面写入字面参数与环境值，Build 仅生成对应产物，Run 仅启动该产物一次。
- Rust 调试：确切 Cargo 产物、两路真实 CodeLLDB 暂停、会话选择及原生插件管理退役清理。
- 项目共享：两个隔离项目加载配置并运行；本机字段不随共享传播。

## 夹具与边界

没有改动插件包源码或公开契约；复用先前通过独立 SDK 构建的包并再次核对 SHA-256。真实用例安装到隔离测试目录，不更新用户现有安装。

| 夹具 | SHA-256 |
| --- | --- |
| `dist/plugins/terminal.zip` | `1dc0634d3b38bf630888067fda10943b69cbb9a4c1bfc186361fa0cce48c250b` |
| `dist/plugins/rust.zip` | `4300e070832a174c6835b0517fb889e6ba36bdb91aa1ed31599aa7497c28bf03` |
| `dist/plugins/run-target-example.zip` | `fbb9802a9b100b136363bac68fabfdab0d9477ab073a46039ea1c23d85b0dfbc` |
| `dist/plugins/rust-debugger.zip` | `e006b0831411f65d48479c1071d4d2fec80beab44a732c5b67d820f51c3437fa` |
| CodeLLDB 1.12.3 Windows VSIX | `a916e509308dac817732f63ca604a8b93ed29cd16f38a2fa9f0b64ed58e8f51a` |

Cargo 首次复用旧隔离候选缓存时误读旧 `editor-core` 产物；仅清理该包的构建缓存后重新编译，后续检查及回归通过。编译仍有已有 unused/private-interface/dead-code 与 LNK4217 警告，未作为本轮无关修复范围。

Cargo 自动移除了开始前已有的 unused patch 记录；所有 Cargo 命令完成后恢复原 `Cargo.lock`，工作副本 blob 保持 `17d6f7ba2dc56b0a919410431e1871430186f097`，未引入依赖变更。

UI 结果来自 GPUI VisualTestContext 的原生字段、绘制布局、键鼠事件和组合输入协议，真实包用例实际运行原生进程与调试器。本会话没有可用的原生桌面操控接口，因此未重新人工观察 B3 系统输入法候选窗口、实际窗口的逐项视觉效果；[B1 记录](run-debug-build-ui-fix-2026-10-06.md) 中的人工检查仅是历史证据。

首次 B3 实现验收时，按此前运行分支的授权启动了 `target/debug/editor-app.exe`，参数指向本分支工作区，并确认进程持续运行；没有关闭其他工作区的编辑器进程。该阶段 EXE 的 SHA-256 为 `a6e028f510eede349e63ed74027e3407a51f0bbd1bee7a7cd6f3cfbe9ac9d0c3`。没有重新打包 `dist/editor/`；进程启动结果不等同于人工视觉验收。当前后续版本见下节。

## 同日后续：与插件管理相同的原生置顶窗口

用户明确要求配置窗口与插件管理使用相同的置顶方式。本次替换最初 B3 的主窗口内弹层，复用 `app/dialog.rs` 的 `open_dialog_sized`、原生标题栏与 `WindowKind::Dialog`。Windows 后端为此窗口设置所属父 HWND 并暂时禁用父窗口输入；它位于所属父窗口上方，可拖动至编辑器边界外，行为与插件管理一致，不扩大为多个编辑器实例。

实现位于 `run/ui/dialog.rs`。窗口创建／激活放在 EditorApp 更新租约结束后：首次同步创建曾触发 `cannot read EditorApp while it is already being updated`，相关原生交互用例复现后通过延迟创建修复。初始字段在实际配置窗口中创建，焦点和输入几何属于其 HWND；主编辑窗口不再绘制配置表单。

重复打开仅激活同一窗口、保留草稿；详情与切换确认仍在该配置窗口内。原生标题栏关闭与系统关闭请求先取消当前详情或切换决定；整体关闭由原生窗口实际关闭回执清理草稿及菜单。保存和取消则撤销草稿后延迟移除其窗口。关闭后的旧回执不能清掉新窗口的状态。插件目标默认值、准备绑定及运行流程保持原契约。

自动化现在直接操作生产创建的配置窗口；关闭后切回仍存活的主窗口。新增两项用例覆盖窗口复用及标题栏关闭、系统关闭请求与关闭回执清理。上述首次 B3 的表格和四项真实插件用例是前一阶段结果；本次窗口改造的实际检查如下，未将旧真实包用例写作新窗口置顶的真机证据。

| 检查 | 结果 | `target/run-config-design/` 日志 |
| --- | --- | --- |
| `cargo fmt --check` | 通过 | 终端输出 |
| `cargo test --workspace --exclude editor-app` | 175 passed、0 failed、160 ignored | `native-window-non-ui.log` |
| `cargo check --workspace` | 通过 | `native-window-check.log` |
| `cargo test -p editor-app run::run_ui_tests -- --test-threads=1` | 32 passed、0 failed、0 ignored | `native-window-ui.log` |
| `cargo test -p editor-app -- --test-threads=1` | 371 passed、0 failed、118 ignored；含上述 32 项 | `native-window-app-all.log` |
| `cargo build -p editor-app` | 通过 | `native-window-build.log` |
| `website/` 中 `node --test tests/*.test.mjs` | 17 passed、0 failed、0 skipped | `native-window-website.log` |
| `git diff --check`、文档链接、原有锁文件逐字节回读 | 通过 | 终端输出 |

应用测试仍使用进程级 `RUST_MIN_STACK=16777216`。`Cargo.lock` 已恢复为本次开始前的相同字节，blob 仍为 `17d6f7ba2dc56b0a919410431e1871430186f097`。只在 `Editor-run-debug-build` 修改，未提交、推送或合并主工作区。

新编译 EXE 为 `target/debug/editor-app.exe`，SHA-256 为 `b37d8c8dfd1d0a2607669d7d3e2de99af0596735e12cc932917170b6a6d468c1`；已按已有授权启动，参数指向本分支，未关闭其他工作区程序。`dist/editor/` 未重新打包。系统置顶与父窗口输入禁用使用与插件管理相同的原生代码路径；本会话仍无法人工操控并核对实际 Windows Z-order、拖动、UIA 与系统 IME 候选窗口，不将自动化窗口生命周期验证替代为这些真机结论。

## 顶栏配置下拉精简（2026-10-06）

按用户补充要求，菜单由按钮实际布局边界定位到其正下方，不再跟随点击位置。上半部分只显示已保存配置，空列表显示禁用的浅灰色「暂无配置」；下半部分由分割线隔开，始终提供「编辑配置」，空列表也可进入配置窗口。插件默认目标及发现入口继续位于配置窗口。缺失目标警告和配置选择行为保留，中英文读者文档同步更新。新空态用例在深浅主题中点击实际按钮，并验证 Enter 跳过禁用行打开独立配置窗口。

| 检查 | 结果 | `target/run-config-design/` 日志 |
| --- | --- | --- |
| `cargo fmt --check` | 通过 | 终端输出 |
| `cargo test -p editor-app run::run_ui_tests -- --test-threads=1` | 34 passed、0 failed、0 ignored | `selector-ui.log` |
| `cargo test -p editor-app ui::controls::menu -- --test-threads=1` | 2 passed、0 failed、0 ignored | `selector-menu.log` |
| `cargo test --workspace --exclude editor-app` | 175 passed、0 failed、160 ignored | `selector-non-ui.log` |
| `cargo check --workspace` | 通过 | `selector-check.log` |
| `node --test website/tests/*.test.mjs` | 17 passed、0 failed、0 skipped | `selector-website.log` |
| `cargo build -p editor-app --release` | 通过 | `selector-release.log` |

本次最新程序为 `target/release/editor-app.exe`。此前已启动的 `dist/editor/editor-app.exe` 仍是上一版，未强制关闭用户窗口或覆盖运行中的可执行文件。原生交互自动化使用 GPUI 测试平台；实际 Windows 显示效果仍需人工核对。只修改分支工作区，未合并主分支。

## 发现按钮移至顶部工具栏（2026-10-06）

按用户标注，将左侧列表底部的「发现配置」按钮移至顶部、加号左侧，仅显示搜索图标，保留国际化悬浮提示、可访问名称及原有插件发现回调。窄窗口同样显示该入口。设计文字与中英读者指南同步；历史设计截图保留，不作为当前实现证据。

运行界面测试 34 passed、0 failed、0 ignored，新增宽窗口及窄窗口（中英、深浅主题、放大字体）下发现按钮与加号同一行的布局断言。`cargo fmt --check`、`cargo test --workspace --exclude editor-app`、`cargo check --workspace`、`cargo build -p editor-app --release` 均成功，读者文档测试 17 passed、0 failed。日志为 `target/run-config-design/discovery-header-{ui,non-ui,check,release,website}.log`。最新 EXE 为 `target/release/editor-app.exe`；没有强制重启已打开的旧窗口，也未合并主分支。真实 Windows 可见效果仍需人工查看。

## 顶栏选择按钮对齐与留白（2026-10-06）

按用户截图反馈，文字使用 14px 内容高度和行高，箭头使用同高的 14px 图标盒；按钮高度 26px、水平 padding 5px。新增项目自有细箭头 SVG（24px viewBox、1.5px 描边），随应用嵌入并继承主题颜色。保留按钮的键盘、焦点、悬浮提示和下拉锚点行为。深浅主题下的生产布局测试断言文字与箭头垂直中心一致，以及左右、上下四个内容边距相等（允许 0.5px 测量误差）。

`cargo test -p editor-app run::run_ui_tests -- --test-threads=1`：34 passed、0 failed、0 ignored；`cargo fmt --check`、`cargo test --workspace --exclude editor-app`、`cargo check --workspace`、`cargo build -p editor-app --release` 均成功。日志为 `target/run-config-design/selector-spacing-{ui,non-ui,check,release}.log`。最新 EXE 位于 `target/release/editor-app.exe`，未自动关闭旧程序或合并主分支。自动化验证布局几何，实际 Windows 字体栅格化及视觉效果仍需人工核对。
