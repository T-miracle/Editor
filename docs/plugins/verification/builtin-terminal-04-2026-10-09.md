# 内置终端第四阶段与整批验收（#91）

范围：N18–N21、旧终端发行退役，以及四张工单的有效证据汇总。累计审查基线为 `48e33ba1f6e8232349206b171256b66fac1fd901`；03 正式提交为 `5fb22821a25992f5dc549f79a306b11311e9b818`；最终代码复审快照为 `c5f8dd6001f754090e9ca2a854b4ca7f06e657c2`。文档交付增量另行只读检查，快照不移动正式分支。

## 升级与发行行为

- 应用在加载配置、布局和插件实例之前执行有限迁移。仅识别已退役的 `terminal` / `me.terminal`、版本不超过 0.12.2 且组件为 `terminal.wasm` 的安装；同 ID 的现行包及其他插件不进入该路径。注册记录先归档，旧访客不执行。
- 通过运行时现有的工作区私有数据解析读取旧状态，转换旧核心的可见行、软换行、历史和光标，保留名称、顺序、Shell、cwd、配置、主题、侧栏及面板布局。旧运行和调试 Tab 恢复为已停止，普通 Shell 只在信任允许时重新创建；不重放旧命令。
- 原始状态、设置、运行配置、布局和提供者选择先做不覆盖备份。五类固定写入目标使用完整校验的有界计划、前后摘要、同步及原子替换；中断可恢复，新的用户数据拒绝覆盖。损坏输入或日志会显示错误，并阻止受影响的布局和配置自动保存。
- 旧 Shell 配置迁往内置提供者，保留原 ID、参数、事件和顺序。配置标记为需重新校验，经普通配置表单保存后才执行，迁移本身不提供执行授权。既有 `MeEditor` 持久化标识不变。
- 旧快照允许原格式已接受的大小：外层 40 MiB、解码数据 16 MiB。新状态限制为 8 MiB，超限仅从最旧历史裁剪，不删当前画面、名称或配置；恢复和续写按同一容量规则处理。
- `plugins/terminal/`、旧核心依赖、终端专用 WASM 测试及独立构建/发行入口退役。保留其他插件使用的公开执行、PTY、面板和配置 API。上游 Alacritty 0.26.0 仍直接由宿主依赖，许可证加入原生发行复制步骤。

## 恢复与原生重排修复

ConPTY 只知道本次真实进程的画面；它的绝对定位和 resize 重绘不能覆盖恢复历史或上一准备步骤。旧内容保存在正常历史中，新进程拥有独立的实时屏幕。初始化尺寸测量在首次继承查询前仍采用 Alacritty 重排，继承后实时屏幕由 ConPTY 重绘，离线历史由 Alacritty 重排；真实 EOF 后恢复普通重排。全屏 alternate screen 保持上游行为。

恢复替换所有单元格，防止短行叠加旧后缀；先用默认模板构造历史，再恢复实时渲染模板，防止新 SGR 背景污染旧行。每个新进程重置 VT 和元数据解析状态。保留带样式的空白及宽字符，鼠标报告将可见行映射回真实屏幕并拒绝历史区。相同尺寸在复制历史之前返回。以上改动均有最小回归，长路径真实 PowerShell 在恢复后继续输入并连续改变宽高，旧历史和提示符数量保持。

## Windows 组合操作

当前调试产物 SHA-256：`682b5403f7b7a943da54ea1bceaedc36fed3d9f2f3f4516f910aec5709b26f91`。隔离根为 `%TEMP%/Nanobug-Upgrade-QA-50b7cfb5aabb488d3cb65fe23ccc3a6e`，未修改用户真实历史。

1. 通过显式的 `prepare_builtin_upgrade_acceptance_profile` 准备真实旧安装记录、两个旧 Tab、旧状态和配置；仅安装当前配置提供者和调试器，没有终端包。应用启动后只有一个内置终端，两个 Tab、`OLD_USER_HISTORY` 和两条原有提示符可见，旧任务保持停止。
2. 在实际 Windows 窗口拖动底部面板高度，最大化并还原总窗口，再缩小和扩大终端高度。长 cwd 随宽度折叠／展开，仍只有两条提示符及一份旧历史，无恢复横幅或额外空白行。
3. 通过实际配置弹窗保存迁移配置，点击工具栏构建，得到 `UPGRADED_RUN_OK`；随后运行同配置，复用同一个第三 Tab，旧输出清除，新结果可见。普通 Shell 和旧任务 Tab 不被占用。
4. 通过工具栏选择并调试 `Interactive Debug`，创建第四 Tab；检查区在该 Tab 内。实际点击终端输入区，输入 `7`、Enter，得到 `INPUT_RESULT:15`。`starts.txt` 只有一次启动，PID 为 56424；停止后该 PID 已退出，输出继续保留。
5. 正常关闭并重新启动最终产物，四个 Tab 的名称和输出保留，旧任务都显示结束，启动记录仍为一条，原 PID 不存活。确认迁移备份和完成标记存在，运行时注册表仅有配置提供者与调试器，原终端记录未再次加载。最后正常退出验收实例。
6. 再次打开同一最终产物，切回恢复的 Shell 并聚焦命令输入区，通过本机 Windows 中文拼音输入法逐键输入 `nihao`。观察到光标旁的预编辑文本和“你好”候选窗口，空格提交后终端回显“你好”，光标位于文字末尾；再输入 `n` 并用 Esc 取消，只保留此前中文，两次退格依次删为“你”和空输入。未执行测试文字，正常退出隔离实例（PID 35236）。

这些是 Windows 真实窗口操作；输入法使用本机系统候选窗口，键盘事件由自动化工具发送。GPUI 模拟组合状态与真实包测试在下表单独列出，不冒充物理键盘设备测试。

## 检查结果与有效范围

环境：Windows、Rust 1.95、已安装 MSVC 14.50 和 Windows SDK 10.0.26100。只在子进程设置 `RUST_MIN_STACK=16777216` 与现有库的 `LIB` / `LINK`，未安装工具链或更改全局环境。构建和包准备直接调用 Cargo、宿主 SDK 入口及归档工具，未使用旧脚本。

| 命令或入口 | 实际结果 |
| --- | --- |
| `cargo test -p editor-app -- --test-threads=1` | 491 通过、0 失败、182 ignored，138.83 s；未执行项不计通过 |
| `cargo test -p editor-app terminal:: -- --test-threads=1` | 最后颜色修复后 32 通过、0 失败、2 ignored，28.46 s；覆盖最终恢复、迁移、任务及真实 Shell resize |
| `cargo test --workspace --exclude editor-app` | 229 通过、0 失败、181 ignored；最终只改 editor-app 的恢复顺序后不重复未受影响的非 UI 门禁 |
| `cargo fmt --check`、`cargo check --workspace`、`cargo build -p editor-app` | 最后恢复修复后通过；最终 GUI 使用上述构建哈希 |
| `cargo test -p plugin-runtime --test host_execution -- --ignored --test-threads=1` | 16 通过，635.88 s；实际公开消费者、受控会话和退休 |
| `cargo test -p plugin-runtime --test interactive_execution -- --ignored --test-threads=1` | 第一轮 12 通过、1 回执等待失败；改为等待公开停止回执后，重跑该项 1 通过，94.84 s；不是第二次完整 13 项运行 |
| `cargo test -p plugin-runtime --test execution_lifecycle -- --ignored --test-threads=1` | 6 通过，8.43 s |
| `cargo test -p editor-app native_configuration_tests -- --ignored --test-threads=1` | 当前 Alpha/Beta 配置包经原生编辑、应用/取消、保存、重开与执行，3 通过，82.34 s |
| `cargo test -p editor-app native_debug_stdin_reaches -- --ignored --test-threads=1` | 当前恢复修复前的最终候选，1 通过，33.65 s；程序输入、唯一目标与同 Tab 检查 |
| `cargo test -p editor-app native_rust_debug_controls -- --ignored --test-threads=1` | 同候选 1 通过，33.01 s；断点、步入、检查、关闭取消与停止 |
| `cargo test -p editor-app a_restricted_workspace_refuses_to_launch_from_the_run_control -- --ignored --test-threads=1` | 1 通过，28.96 s，现行配置包经运行控件拒绝执行 |
| `cargo test -p editor-app terminal::tests::fullscreen -- --ignored --test-threads=1` | 最后恢复修复后，真实 Neovim、中文/emoji、alternate screen 与 resize，1 通过，2.32 s |
| `prepare_builtin_upgrade_acceptance_profile` | 显式隔离根的准备测试 1 通过，26.67 s；本项只准备数据，不冒充 GUI 验收 |
| `npm test`（当时的 website） | 文档源 17 通过；此后工作区有独立文档目录整理，站点迁移不属于本单，不以旧结果验证其改动 |
| `git diff --check` | 本单检查通过 |

最后颜色修复只改变保存单元格的构造顺序，由整个终端批次和当前 Neovim、实际升级/调试组合场景覆盖；没有改变公开协议、消费者、配置表单或 DAP 传输。故不再重复耗时的全部公开包测试。日志保留于 `%TEMP%/nanobug-t04-*`；最终门禁为 `editor-delivery`、`workspace-final`、`fmt-current`、`check-current`、`build-current`，增量为 `terminal-color-green`、`fullscreen-current`，调试和信任为 `debug-native-current`、`restricted-current`。

当前 SDK 构建产物：`capability-example` 0.17.1（SHA-256 `ee67e56f0a5186265ead02cd634155ea4df938deb6fc12cab2d6b9f5f70420cb`）、`configuration-example` 0.1.2（`0a31fdcb44b13253b1f0b1c4b25f3e32f4beb36e97438e2e8915b25c54fc5b3a`）。原生配置接受同组件的 Alpha/Beta 两个普通包身份，没有测试专用宿主接口。CodeLLDB 1.12.3 VSIX 的 SHA-256 为 `a916e509308dac817732f63ca604a8b93ed29cd16f38a2fa9f0b64ed58e8f51a`；调试提供者沿 03 的公开契约构建。

## 失败与限制

- 初次应用并行批次发生共享注册表竞争及本单旧测试清单的 bounds 表达错误；改用串行门禁并修复清单。后一轮串行仅剩已退役终端身份的 dock 夹具失败，改为普通现行包身份后，最终 491 项全通过。
- 真实长路径恢复 resize 曾丢旧历史、重复提示或覆盖上一步输出；已通过真实回归确认失败再修复。历史背景污染同样先观察失败，再由 50 多行历史和后续 SGR 输出的回归确认修复。新测试最初误用未实现的 `SavedGrid::clone`，是夹具编译错误，不计产品 RED。
- 一次旧原生验收组合为 1 通过、16 失败（`native-final`，186.52 s）。其中 3 个现行配置测试及受限工作区测试因缺夹具失败，直接准备当前 SDK 包后已重跑通过。剩余 12 项历史构建／发现／旧 Run 测试仍使用基线已拒绝的旧配置结构、已弃用的表单选择方式或未准备的历史包；这些失败未报告通过，也未为它们新增生产旧协议兼容。当前任务行为由现行终端、配置、公开消费者和实际组合场景覆盖；历史夹具升级仍是已知测试债务。
- macOS/Linux 未在本机编译或运行。Windows 中文拼音的系统候选窗口、提交、取消与退格已实际验收；其他输入法、物理键盘设备差异未验证。中文组合状态另复用 01 的 GPUI 回归，中文与 emoji 的真实 TUI 文件内容在本单重测。未发布安装器或版本；交付为源码、现有直接打包入口及 Windows 原生调试产物。
- 既有 unused/private-interface 和链接器警告保留。工作区独立的社区平台、文档目录与其他改动不纳入本单提交或通过声明。

## N01–N26 证据汇总

| 编号 | 本单补测或仍有效的前单证据 |
| --- | --- |
| N01 | 01 无包打开真实 Shell；04 原生升级产物与实际注册表再次确认不依赖终端包 |
| N02 | 01 空面板/最后 Tab 生命周期；最终应用门禁仍通过相应用例 |
| N03 | 03 同面板；04 四个实际 Shell/构建/运行/调试 Tab 组合 |
| N04 | 02 准备顺序、失败/取消；04 终端任务批次补测跨原生步骤输出保留 |
| N05 | 02 实际 ZIP 打包、开发实例、重载和停止；04 退役发行入口及迁移配置构建 |
| N06 | 02 两个真实 PTY 隔离；04 最终任务批次及不占用普通 Shell 的实际组合 |
| N07 | 02 普通重复定位、显式重跑等待退出；04 最终任务批次 |
| N08 | 02 复用/关闭后新建、旧事件隔离；04 实际 Build/Run 复用第三 Tab |
| N09 | 02 关闭确认/取消与真实树退出；03 调试关闭；04 最终任务批次 |
| N10 | 02 准备关闭与饱和退休；03 启动间隙关闭；04 最终生命周期消费者 |
| N11 | 02 隐藏、定位与结束关闭；04 实际停止保留输出，重启后非运行状态 |
| N12 | 03 按选中 Tab 组合检查；04 实际第四 Tab 内显示调试检查区 |
| N13 | 03 真实唯一目标与断点；04 原生 stdin 回归、实际输入结果 15、一次 PID 启动 |
| N14 | 03 真实步进/多会话/暂停代次；04 原生调试控制回归，检查状态未迁移到第二处 |
| N15 | 03 真实失败/超时/退休树清理 7 项；04 调试协议未改，实际停止 PID 已退出 |
| N16 | 01 损坏快照不写、真实权限撤销；02/03 公开资源归属与信任；04 当前配置拒绝执行及安全迁移 |
| N17 | 02 双身份公开消费者；04 host_execution 16 项、interactive_execution 12+1 项与现行配置包 |
| N18 | 04 真实旧格式、源备份、布局/配置转换和 Windows 升级操作 |
| N19 | 04 损坏状态、日志预校验、中断恢复、新数据冲突、重复执行和保存保护回归 |
| N20 | 04 工作区隔离、保留其他插件及同 ID 当前包、真实重启四个 Tab 且启动记录不增加 |
| N21 | 04 原生长路径恢复/继续输入/四次 resize、容量和颜色回归；实际反复宽高操作 |
| N22 | 01 原生复制/选择/菜单、GPUI IME；04 全应用、实际调试输入，以及 Windows 拼音预编辑/候选/提交/取消/退格 |
| N23 | 01 SideTabs 编辑/排序/侧栏与共享滚动条；04 完整应用回归、恢复侧栏宽度及实际名称顺序 |
| N24 | 01 深浅主题/中英文/字号与 Canvas；04 保留主题/布局、颜色回归及实际缩放/光标 |
| N25 | 01 常用 Shell/混合宽字符；04 当前真实 Neovim、alternate screen、Unicode 与 resize 重测 |
| N26 | 02 离开确认及进程清理、03 调试退休；04 实际正常退出/重开且不重放任务 |

前单记录：[01](builtin-terminal-01-2026-10-09.md)、[02](builtin-terminal-02-2026-10-09.md)、[03](builtin-terminal-03-2026-10-09.md)。各项复用范围以代码、依赖、契约和输入未受后续修改影响为前提；没有把被忽略的测试或未运行平台记为通过。

## Standards

[code-review](C:/Users/Tmiracle/.agents/skills/code-review/SKILL.md) 规范轴先后指出并修复迁移保存保护旁路、结束后仍走原生裁剪、跨进程解析状态和相同尺寸下重复复制历史。最终 `c5f8dd6` 硬性违规 0、判断性坏味道 0。审查为静态只读，执行与 GUI 证据来自上述主流程。

## Spec

规格轴先后指出并修复旧容量、同 ID 当前包误导入、保留历史时鼠标坐标错误、样式空白丢失及背景污染。最终 `c5f8dd6` 无剩余确定代码缺陷或范围蔓延；文档复审要求补充真实 Windows IME，已按上面的第 6 步完成。规范轴发现的一处终端包 ignore 前提也已同步为当前 Rust/调试器包。验收后执行普通提交、推送和 #91 状态读回；父设计议题 #87 保持不变。
