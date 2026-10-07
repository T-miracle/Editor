# 快捷键面板阶段验证记录

日期：2026-10-07

状态：全部三单已交付。`495a1d0`、`66fb23a`、`9d91893` 已推送主分支，#81、#82、#83 均读回为 `closed` / `completed`。最终自动验证、两轴审查及必要原生组合验收通过；父规格 #77 读回仍为 `open`，本次未改写或关闭。

规格：[快捷键面板与用户绑定](../specs/keyboard-shortcuts.md)。工单：[01 → 02 → 03](../tickets/keyboard-shortcuts/README.md)，对应 [#81](https://github.com/T-miracle/Editor/issues/81) → [#82](https://github.com/T-miracle/Editor/issues/82) → [#83](https://github.com/T-miracle/Editor/issues/83)。父规格 #77 保持原样且不关闭。

## 工作树与证据范围

- 工作区：`C:/Projects/RustProjects/Editor-shortcuts-main`；用户要求使用 `main`，基线提交为 `ae63225afd6e8a362c6f186d7374323e97405469`。
- 工单 01、02、03 分别对应交付提交 `495a1d0`、`66fb23a`、`9d91893`；03 两轴审查以 `66fb23a` 为固定点。各阶段结果只证明当时执行的场景；03 最终全量应用测试另行记录，不将历史记录冒充当前完整重跑。
- 构建复用 `CARGO_TARGET_DIR=C:/Projects/RustProjects/Editor/target`。复用产物不表示把原工作区未提交源代码迁入新工作区。
- 沿已确认的 GPUI 应用入口，通过按键、点击、可见布局、焦点和真实文件内容验证行为。没有以目录模型的内部单元测试代替应用级行为。
- 本记录依据主执行任务运行后保留的日志核对；整理记录时没有重新执行 Rust 构建或测试。

## 已执行的红绿迭代

| 阶段 | 执行与结果 | 证据及限制 |
| --- | --- | --- |
| 打开与焦点恢复 | `tests::shortcuts::shortcuts_open_and_restore_focus`：1 passed、0 failed、0 ignored | `shortcuts-stage.log`；模拟 Ctrl+K 打开，只保留一个应用窗口，Esc 关闭后原编辑器重新获得焦点。未验证完整主题、IME 或插件生命周期。 |
| 搜索测试首次运行（红） | `tests::shortcuts::shortcuts_search_and_capture_without_running_commands`：0 passed、1 failed、0 ignored | `shortcuts-search-red.log`；失败为 `visual.debug_bounds("shortcuts-search").is_some()`，当时弹层没有搜索控件，证明新增场景先观察到缺失行为。 |
| 查询实现后的针对性验证（绿） | `cargo test -p editor-app tests::shortcuts -- --nocapture`：2 passed、0 failed、0 ignored，507 filtered out | `shortcuts-search-green.log`；上述打开测试和搜索/按键捕获测试均通过。过滤掉的测试没有报告为通过。 |

## 阶段 01 最终针对性验证

| 执行 | 结果 | 日志 |
| --- | --- | --- |
| `cargo test -p editor-app tests::shortcuts -- --nocapture` | 4 passed、0 failed、1 ignored、507 filtered out；该子串也匹配插件查询测试，因此有 1 个待显式执行的 ignored | `shortcuts-stage1-ui-final.log` |
| `cargo test -p editor-app shortcuts_real_plugin_query_and_capture_do_not_execute -- --ignored --nocapture` | 1 passed、0 failed、0 ignored、511 filtered out | `shortcuts-stage1-plugin.log` |
| `cargo test --workspace --exclude editor-app` | 196 passed、0 failed、167 ignored；汇总 50 个结果块，其中 5 个 doc-test 组均为 0 tests | `shortcuts-stage1-workspace.log` |
| `cargo check --workspace` | 成功，退出码 0；日志为 `Finished dev profile` | `shortcuts-stage1-check.log` |
| `cargo fmt --check` | 成功，退出码 0；正常无输出 | `shortcuts-stage1-fmt.log` |
| `cargo build -p editor-app` | 成功，退出码 0；日志为 `Finished dev profile` | `shortcuts-stage1-build.log` |

常规应用测试中的 1 个 ignored 是下行单独执行的真实插件场景，不能在常规组中算作通过。workspace 的 167 个 ignored 未在本阶段全量执行；本次显式插件测试也不代表这些其他集成场景通过。

主执行任务确认非 UI 测试进程退出码 0；最后的串行执行链仅在前项成功后运行下一项，包含针对性测试、显式插件测试、格式检查、workspace 编译和宿主构建，最终退出码 0。格式检查成功依据进程结果，不是根据空日志推断。

四项应用测试覆盖的可观察行为：

- Ctrl+K、按钮和真实应用菜单均能打开弹层；只保留一个窗口，Esc 关闭后恢复原编辑器焦点。Tab 与 Shift+Tab 在搜索框失焦后仍能移动焦点。
- 打开后搜索框拥有焦点；Alt+Right 切换 tab 保留文字条件；固定面板大小。按键录入 Ctrl+S 可查到保存操作，但脏文档的磁盘内容仍为原文；第一次 Esc 退出录入，第二次关闭。
- 两步快捷键先按前缀显示候选，再按完整序列和精确修饰键过滤；滚动时 tab 与底部位置不变；小窗口与 1.5 倍缩放下弹层不越界；已提交的中文输入可查询空结果；点击 mask 关闭。
- 未绑定的 `DeleteToBeginningOfLine` 可见；重新开始录入保留完整等待时限，旧计时器不提前截断新序列；录入中的 Alt+Left 是待查询按键，等待 2 秒后再次输入按新序列处理；从文件树打开时不误继承弹层搜索框的 Input 上下文。

审查后修复了 `Root` 谓词动作的分类，使其进入全局清单；隔离背景动作时保留 `root::Tab` / `root::TabPrev`，避免误拦截上游焦点遍历。录入状态清空后继续递增 generation，防止旧超时任务影响新录入。上述最终应用测试在这些修复之后执行。

参数化 Enter 的普通、Shift、辅助三个描述均通过真实搜索框独立查到，目录保留原动作参数。以上 GPUI 场景使用默认英文资源；原生验收使用中文资源。

## Windows 原生验收（01）

使用从新工作区构建的 debug 程序，在专用 `target/shortcut-native` 工作区打开 `lookup.txt`，插件根通过现有 `ME_EDITOR_PLUGIN_HOME` 指向独立目录。没有操作用户正在运行的 release 编辑器或其他工作树验收进程。会话数据仍按应用原有规则保存为此专用工作区的记录；没有把 Windows 的 APPDATA 环境覆盖误称为完整 profile 隔离。

- 实际 Ctrl+K 打开主窗口内弹层，搜索框可直接输入“复制”，列表显示复制及 Ctrl+C。
- 切换已安装的中文输入法后，真实字母键产生候选栏，按数字 2 提交“发”；提交进入搜索框并更新空结果。这一结论来自原生候选和提交观察，不以 Unicode 文本注入冒充 IME 组合。
- Alt+Right 切换全局 tab 并保留文字；按钮开启按键搜索后 Ctrl+S 显示保存操作与按键，源文件未变化。第一次 Esc 退出录入并保留弹层。
- 原生滚轮只改变列表内容，tab、搜索区与底部提示位置保持不变；点击背景遮罩关闭并恢复编辑器。
- 浅色和深色主题的弹层、键帽、边框与文字均可见。原生发现遮罩 token 自带透明度，原来的乘法造成过淡；改为 `alpha(0.55)` 后重新构建并检查明确遮罩。
- 小窗口及 1.5 倍缩放由 GPUI 应用场景验证；本次没有修改 Windows 显示缩放设置。

实际截图：[浅色与滚动](assets/keyboard-shortcuts-light.png)、[深色与全局清单](assets/keyboard-shortcuts-dark.png)。截图是原生实现；规格中的 C 图仍是设计原型。

## 真实 WASM 插件夹具

已成功执行 `./scripts/build-capability-example.ps1 -HostExe 'C:/Projects/RustProjects/Editor/target/debug/editor-app.exe'`，退出码 0。沿公开宿主 SDK 构建，使用已安装的 `wasm32-wasip2`，没有安装额外工具。脚本的夹具产物位于新工作区的 `target/plugin-api-test/capability-example.zip`，大小 507462 字节，SHA-256 为 `F7B36538954C71CD430019F434901B3B57E86C6A1DE9D3A8B86807DDEB8FE2C1`。

显式 ignored 测试保留真实 component 和资源，仅改清单为宿主未知的插件 ID，声明一个有绑定命令和一个无绑定命令，并将面板初始设为隐藏。包经公开 `Manager::install`、发布和宿主应用进入清单：两条命令均可文字查询；普通搜索和按键录入中按插件快捷键，实际 pump/publish 后隐藏面板仍不存在；关闭弹层后按同键，真实组件显示面板作为正对照。测试没有以私有工作队列或新增测试专用宿主 API 代替执行结果。

## 阶段 02 最终自动验证

以下结果来自完成绑定修改、统一分发与审查修复后的工作树。两轴最终审查（Standards / Spec）以 `495a1d0` 为固定点，未留下 P0、P1 或 P2 问题。自动验证与下节原生重启验收分别记录，不代表工单 03 已验收。

| 执行 | 结果 | 日志 |
| --- | --- | --- |
| `cargo test -p editor-app shortcuts -- --nocapture` | 20 passed、0 failed、2 ignored、507 filtered out；包含应用场景与配置/解析器边界测试 | `shortcut-02-targeted-final.log` |
| `cargo test -p editor-app shortcuts_real_plugin -- --ignored --nocapture` | 2 passed、0 failed、0 ignored、527 filtered out | `shortcut-02-plugin.log` |
| `cargo test --workspace --exclude editor-app` | 196 passed、0 failed、167 ignored；汇总 50 个结果块 | `shortcut-02-workspace.log` |
| `cargo check --workspace` | 成功，退出码 0 | `shortcut-02-check.log` |
| `cargo fmt --check` | 成功，退出码 0；正常无输出 | `shortcut-02-fmt.log` |
| `cargo build -p editor-app` | 成功，退出码 0 | `shortcut-02-build.log` |

20 个通过项包含 10 个配置/解析器边界测试和 10 个 GPUI 应用测试。常规组的 2 个 ignored 为随后显式执行的真实插件查询及编辑场景；workspace 的其他 167 个 ignored 仍未全量执行。编译与链接警告仍存在，未将通过写成无警告。

### 应用行为与存储证据

- 行内新增、修改、删除、多绑定和恢复默认通过面板实际按钮完成；用磁盘文件观察保存动作，验证新键立即生效、旧键失效、未修改的同操作其他绑定保留。Ctrl+K 的打开绑定自身可修改、删除，真实应用菜单在删除后仍可打开面板并恢复默认。
- 在改绑前创建两个不同工作区的真实 `EditorApp` 窗口及一个独立 `Base Root` 输入窗口。面板内修改 `input::Copy` 后，单步与两步绑定均立即作用于三个窗口；普通快捷键搜索输入框也使用新绑定。原 `Ctrl+C` 在 Input 选择文本时保持剪贴板哨兵值，证明没有通过祖先 Root 的同名 Copy 动作误走旧输入复制路径；Root 的 UI 文本复制能力仍保留原作用域。
- 编辑器与独立输入控件保留既有复制、粘贴和撤销行为。`DeleteToPreviousWordStart` 从 Ctrl+Backspace 改绑后，旧键不改文档，新键真实删除单词，Undo 恢复文本，再删除并保存后磁盘内容一致；编辑仍走原有 EditorState / DocumentSession。
- 通过按键搜索准确定位普通 Enter 参数行，只修改该行；独立 Base Input 的真实 `InputEvent::PressEnter` 验证新键传入普通参数，原 Shift+Enter、Ctrl+Enter 分别保留原参数，旧普通 Enter 不再触发。按键查询进入行内编辑时保留筛选，避免编辑行被完整列表挤出可见区。
- 录入与查询捕获期间不执行已保存的单步或两步业务动作；两步首键显示下一步提示，匹配后仅执行一次。超时、未匹配第二步的正常输入、焦点及配置 revision 变化导致的待完成序列取消均有针对性验证。
- 冲突不会在保存时静默覆盖，必须点击“替换冲突绑定”；只撤销冲突组，保留对方其他绑定。恢复默认同样先做冲突预览。未保存草稿在遮罩关闭、tab 切换及其他编辑意图前显示继续/放弃确认；录入态 Esc 先取消草稿，再按 Esc 才关闭。
- 保存按钮在录入结束后获得焦点。应用测试禁用祖先 Dialog 的 Enter 动作，再发送实际 KeyDown 和 KeyUp，确认 Base 按钮仍能键盘提交。早期仅发送 KeyDown 的测试缺少按钮所需释放事件，不能据此宣称生产程序无法通过 Return 保存。
- 用户配置经既有 `NativeFileStore::write_utf8` 原子写入；写入成功后才发布有效绑定。存储失败保留运行中的配置，损坏文件重载报错并保留原文件；默认文本键与新录入文字键校验分别处理。真实临时用户 profile 的重载和另一工作区验证覆盖持久配置共用，没有通过私有 Engine 映射断言替代应用执行。

对应应用测试位于 `crates/editor-app/src/tests/shortcuts_editing.rs`、`shortcuts_controls.rs`；配置及解析器边界测试位于 `crates/editor-app/src/app/shortcuts/engine/tests.rs` 及其子模块。

### 真实插件增量

继续复用阶段 01 的真实 WASM 夹具 `capability-example.zip`，SHA-256 仍为 `F7B36538954C71CD430019F434901B3B57E86C6A1DE9D3A8B86807DDEB8FE2C1`；本阶段未改公开 SDK 或协议，不重复构建该包。两项显式 ignored 测试均通过。

新增编辑场景经公开 Manager 安装与宿主发布，用真实组件显示面板观察命令执行：插件单步改绑后旧键失效；两步首键仅显示等待提示，第二步后面板出现；与宿主 Ctrl+S 冲突时必须显式替换，替换后插件键不再保存文档，宿主另一绑定仍保存。录入中 pump/publish 后插件面板保持隐藏。未引入专用宿主调用路径，也未把这一静态有效插件场景扩大为停用/卸载/恢复的生命周期证据。

### Windows 原生验收（02）

已在实际窗口将保存操作由 Ctrl+S 行内改为 Ctrl+Alt+S，等待录入完成后通过 Return 提交；旧 Ctrl+S 不写入文件，新键写入成功。

最后一次构建后，在同一 `target/shortcut-native-02/profile` 配置位置重启程序，尚未打开快捷键面板前，旧 Ctrl+S 仍不保存，Ctrl+Alt+S 将 `native startup persisted` 写入真实文件。随后打开面板可见持久保存的新绑定，再行内录入相同按键，等待 2 秒并通过 Return 保存成功，焦点回到可见搜索框。`ME_EDITOR_PROFILE_HOME` 在本任务中只指定快捷键配置文件位置，不能视为完整应用 profile 隔离；插件目录独立使用 `ME_EDITOR_PLUGIN_HOME`。

实际截图：[重启后的绑定](assets/keyboard-shortcuts-binding-reloaded.png)、[行内编辑](assets/keyboard-shortcuts-inline-edit.png)。原生编辑使用中文、浅色主题；英文及深色冲突提示由 GPUI 应用场景覆盖，沿用 01 的深浅主题、真实 IME、滚动和缩放证据，没有声称再次手动穷举全部组合。

工单 02 已以 `66fb23a3e1a18749ce99f9ce9824bba65e076e5b` 普通提交并推送 `origin/main`，推送后核对远端提交，并读回 #82 为 `closed` / `completed`。

## 阶段 03 最终自动验证与审查

本阶段沿真实 Manager 的实例身份维护命令 epoch，撤销旧的待完成序列、延迟 UI 调用及工作队列调用。插件退役会刷新已打开的目录并取消失效草稿，保存文件不变；命令恢复冲突时展示保留的自定义绑定和“处理冲突”，继续沿原有显式替换流程。未修改公开协议、SDK 或发行插件包，不引入专用宿主 API。

| 执行 | 结果 | 日志 |
| --- | --- | --- |
| 生命周期新增场景首次运行（红） | 0 passed、1 failed；禁用后已打开的行仍可见，确实观察到待修行为 | `shortcut-03-lifecycle-red.log` |
| `cargo test -p editor-app -- shortcuts status_popup_blocks_title_bar_until_dismissed clicking_outside_an_open_submenu_does_not_press_the_panel_beneath --test-threads=1 --nocapture` | 22 passed、0 failed、5 ignored、505 filtered out | `shortcut-03-targeted-final.log` |
| `cargo test -p editor-app shortcuts_real_plugin -- --ignored --nocapture` | 5 passed、0 failed、0 ignored、527 filtered out | `shortcut-03-real-plugin.log` |
| `cargo test -p editor-app extensions::worker::worker_tests -- --ignored --nocapture` | 3 passed、0 failed、0 ignored、529 filtered out | `shortcut-03-worker.log` |
| `cargo test -p editor-app host_command_reveals_real_panel_and_preserves_arguments -- --ignored --nocapture` | 1 passed、0 failed、0 ignored、531 filtered out | `shortcut-03-host-command.log` |
| `cargo test --workspace --exclude editor-app` | 196 passed、0 failed、167 ignored；50 个结果块 | `shortcut-03-workspace.log` |
| `cargo fmt --check` | 成功，退出码 0 | `shortcut-03-fmt.log` |
| `cargo check --workspace` | 成功，退出码 0 | `shortcut-03-check.log` |
| `cargo build -p editor-app` | 成功，退出码 0；用于下节原生窗口 | `shortcut-03-build.log` |
| **`cargo test -p editor-app -- --test-threads=1`（最终）** | **384 passed、0 failed、148 ignored、0 filtered out** | `shortcut-03-app-full-final.log` |
| `cd website; npm test` | 15 passed、0 failed、2 skipped；跳过项为尚无站点构建索引的搜索测试 | `shortcut-03-website.log` |

本阶段显式执行了 9 个不同的真实 WASM ignored 场景，不能与常规测试中被跳过的项重复计数。非 UI 的 167 个 ignored 与应用的其余 ignored 未全量运行。真实包继续复用上文已构建的 SHA-256 `F7B365…C1` 夹具；这 9 项通过后仅收紧标题栏按钮的外观并补充数字校验断言，没有再改变插件实例、传输或分发逻辑。最终全量应用测试包含该数字断言。

### 全量测试失败的处理

- 最初默认并行执行得到 366 passed、18 failed。应用测试共用 i18n 和语言注册状态，已有 `docs/specs/plugin-api-lsp-verification.md` 记录了串行执行要求；保留 `shortcut-03-app-full.log`，不将该失败记录删掉或写成通过。
- 改为串行后得到 382 passed、2 failed，失败项为上表的两个现有标题栏交互测试。新增入口的完整英文文字挤掉了 800 像素窗口的拖动区，属于本次回归。将键盘和应用菜单入口改为紧凑符号，保留中英文 tooltip、无障碍名称与原焦点逻辑；两个针对性回归随后通过，再执行最终串行全量得到 384 passed、0 failed。修复前记录为 `shortcut-03-app-full-serial.log`。
- Standards 与 Spec 分别以 `66fb23a` 审查，均无遗留 P0/P1/P2；紧凑入口的后续增量也分别复核通过。未把编译警告或其他未执行的 ignored 场景报告为消失或通过。

### 插件生命周期的实际效果

三个新增 GPUI 场景位于 `crates/editor-app/src/extensions/shortcut_query_tests/lifecycle.rs`，使用此前未知的 `shortcut-query-fixture` 包 ID、真实组件、独立存储和正常用户配置：

- `shortcuts_real_plugin_lifecycle_disable_removes_open_rows`：行内改绑后，公开 Manager 禁用即刷新已打开的列表、取消失效录入并显示错误，文件字节保持；重新启用后新键执行。卸载后安装移除原命令的版本时旧键保持惰性，再安装恢复该命令的版本才恢复绑定。
- `shortcuts_real_plugin_lifecycle_restoration_conflicts_require_decision`：深色、英文场景分别验证相同单步和连续前缀冲突。恢复后原保存操作仍写入文件，插件绑定只显示保留警告；取消不写配置，明确替换后插件执行，另一组保存键仍有效。
- `shortcuts_real_plugin_lifecycle_epochs_and_restricted_workspace`：实际 guest 文本 `Typed errors and request IDs verified.` 为命令运行正对照。相同包 digest 的实例重启会取消前缀；旧第二步、旧队列调用，以及实际 KeyDown 解析后、UI 延迟回调前发生的重启都不能进入新实例。新按键仍执行；插件焦点打开面板再关闭后恢复。通过既有应用和 Manager 信任接口转为受限工作区后，查询及宿主改绑不启动插件、不变更信任，保存仍可用且插件绑定保留。

工作线程的三个公开管理器场景和真实宿主调用场景进一步验证有效实例、声明命令、参数、焦点及退役边界。共享 harness 改为发布真实 Manager 状态，不再用模拟 epoch 冒充实例切换。

## Windows 原生组合验收（03）

使用最终构建的 debug 程序与 `target/shortcut-native-03` 专用 workspace、插件目录、快捷键 profile。通过现有 `plugin-runtime/examples/prepare_fixture.rs` 的公开 Manager 准备包，退出码 0；保留原 component 和资源，只修改夹具清单。原生夹具 ZIP SHA-256 为 `993DDBAFAF6381AE3F306A6BA741DF2B6348A63F9B443768A0F96EFF33A1D0F0`。没有手写注册表、操作用户的 release 或其他工作树进程，也没有通过 Windows UI 改变安全/权限设置。

- 启动加载正确的 SaveDocument 稳定标识及保存的插件自定义 Ctrl+Alt+V，中文、浅色界面显示“绑定已保留，处理冲突后才能生效”。点击“处理冲突”后实际列出保存操作 Ctrl+Alt+V，点击“替换冲突绑定”才解除它；配置保留同操作的 Ctrl+Alt+U。
- 关闭弹层后 Ctrl+Alt+V 运行真实插件，面板文本变为 guest 的 `Completed … PanelVisibility … visible: true`。通过正常插件管理器全局禁用后面板消失，同键无效、查询为空，但文件仍保留自定义键；全局启动完成后同键重新执行，没有再次授予权限。
- 在文档中输入 `native lifecycle persisted`，Ctrl+Alt+U 确实写入专用 workspace 的 `native.txt`，证明冲突替换没有损坏另一组保存绑定。
- 同一 profile 重启后，尚未打开快捷键面板便按 Ctrl+Alt+V，guest 再次返回完成结果。随后从插件焦点 Ctrl+K 打开，全局 tab 展示持久绑定和空的聚焦搜索框；Esc 关闭后按 `z` 未进入背景文档，文档内容与磁盘仍一致。验收后正常关闭本任务进程。
- 原生逐步录入的单次尝试因每次截图耗时超过两秒而按单步结束，已取消且未保存；不把它当作两步录入成功。两步实际录入、执行及可控两秒等待由 02/03 GPUI 按键场景证明。01 的真实中文 IME、深浅主题、滚动与小窗口/缩放证据继续复用，没有声称手工重演全部组合。

实际截图：[恢复冲突](assets/keyboard-shortcuts-restored-conflict.png)、[停用后查询](assets/keyboard-shortcuts-disabled-plugin.png)、[重启后插件执行](assets/keyboard-shortcuts-plugin-reloaded.png)。`ME_EDITOR_PROFILE_HOME` 仍只隔离快捷键配置，其他会话按专用 workspace 保存。

## T01–T22 证据对应

下表引用已执行场景，不创建重复测试。01/02 历史证据对应上述交付提交；03 最终 384 项全量重新执行常规应用场景，真实插件生命周期则按上表显式执行。原生部分注明实际阶段。

| 验收项 | 主要应用行为证据与补充 |
| --- | --- |
| T01 | `shortcuts_open_and_restore_focus`、`shortcuts_controls_opener_and_enter_variants`；01 默认入口，02 自定义、删除后菜单兜底与恢复。 |
| T02 | `shortcuts_open_and_restore_focus`、`shortcuts_search_and_capture_without_running_commands`；01 原生 mask 与主窗口。 |
| T03 | `shortcuts_capture_deadline_unbound_and_original_tree_context`、`shortcuts_real_plugin_lifecycle_epochs_and_restricted_workspace`；03 原生插件关闭后的焦点。 |
| T04 | `shortcuts_open_and_restore_focus`、真实插件查询场景；03 原生从插件打开选全局、搜索清空并聚焦。 |
| T05 | `shortcuts_two_stroke_query_and_fixed_layout`、`shortcuts_editing_add_edit_delete_restore_dispatch`；01 原生滚动，最终两个 800px 标题栏回归。 |
| T06 | `shortcuts_search_and_capture_without_running_commands`、`shortcuts_two_stroke_query_and_fixed_layout`；01 原生 Alt+Right 保留查询。 |
| T07 | `shortcuts_search_and_capture_without_running_commands`、真实插件查询和编辑场景；实际保存、剪贴板与插件执行均有正反对照。 |
| T08 | `shortcuts_two_stroke_query_and_fixed_layout`；第一步、完整序列与精确修饰匹配。 |
| T09 | `shortcuts_capture_deadline_unbound_and_original_tree_context`、`shortcuts_editing_unsaved_navigation_and_escape_do_not_persist`；01/02 原生 Esc 优先级。 |
| T10 | `shortcuts_controls_copy_and_document_edits_across_native_roots`、两个真实插件查询/编辑场景；未绑定命令及 Enter 参数行。 |
| T11 | `shortcuts_editing_add_edit_delete_restore_dispatch`、`shortcuts_editing_conflict_requires_replace_and_preserves_siblings`；03 原生保留 Ctrl+Alt+U。 |
| T12 | `shortcuts_editing_add_edit_delete_restore_dispatch`、基础控件实际 KeyDown/KeyUp；02 原生 Return 提交及真实文件。 |
| T13 | `shortcuts_editing_two_step_timeout_typing_and_shared_profile`、跨原生 Root 输入测试；02/03 原生重启前未打开面板就执行新键。 |
| T14 | `shortcuts_editing_conflict_requires_replace_and_preserves_siblings`、03 恢复冲突；原生明确替换及配置文件。 |
| T15 | `shortcut_engine_conflicts_follow_reachable_input_tree_and_plugin_scopes`、03 生命周期恢复的单步/前缀应用场景；不重叠作用域保留。 |
| T16 | `shortcuts_editing_two_step_timeout_typing_and_shared_profile`、真实插件编辑及重启 epoch 场景；等待提示、超时、正常第二步输入。 |
| T17 | `shortcuts_two_stroke_query_and_fixed_layout`、`shortcuts_capture_deadline_unbound_and_original_tree_context`、两步行内保存场景；最多两步校验。 |
| T18 | `shortcut_engine_inherited_text_defaults_reload_but_new_text_is_rejected`；单独/Shift 字母数字、无修饰第二步、三步拒绝，Ctrl 组合及 F9 允许；旧文字默认值仍保留。 |
| T19 | `shortcuts_editing_unsaved_navigation_and_escape_do_not_persist`；遮罩、切换 tab、另一项、继续/放弃与 Esc。 |
| T20 | `shortcuts_real_plugin_lifecycle_disable_removes_open_rows`、受限工作区场景；03 原生禁用后无执行且配置保留。 |
| T21 | `shortcuts_real_plugin_lifecycle_restoration_conflicts_require_decision`、移除命令/恢复版本场景；03 原生恢复警告与显式处理。 |
| T22 | 01 原生中文 IME、深浅主题和滚动，GPUI 小窗口/1.5 倍缩放；02 英文控件与编辑，03 英文深色生命周期冲突及中文浅色原生提示、焦点。 |

## 日志身份

原始日志位于本次实现工作区根目录，属于开发过程证据；以下散列标识本记录读取的版本，后续重跑应新增对应阶段而不是把旧结果当作新结果。

| 文件 | SHA-256 |
| --- | --- |
| `shortcuts-stage.log` | `7CAECC79027E3CF7F74F1B52D46EA6B1918CBED0469B03CCDCD36A3DC6138C2F` |
| `shortcuts-search-red.log` | `F4B5AB1E5AE45AF61EC40C2A5478456776BA134BF8962F664D7BADF677D6D810` |
| `shortcuts-search-green.log` | `96CB43CF64862F912DCD5536AB83A8D4189243CC1A31A23A59245D81A14CECE5` |
| `shortcuts-stage1-ui-final.log` | `E00ED860634CC098C7568DE30F5670E43EA4E7242272D140E77DBE209A456619` |
| `shortcuts-stage1-plugin.log` | `22CCF03D398199A5E51D107D1DC5E74D25DC898881AF2C2D824EC9682CF8B538` |
| `shortcuts-stage1-workspace.log` | `C9949C353F58A510DE2ADFCEE3FD6297003B42F491D0AE64A1CE3B33CBBFE640` |
| `shortcuts-stage1-check.log` | `89CE2E3F87BB9C589E93443145FB0615CB65038620A2788FD4CB42B4A01290BF` |
| `shortcuts-stage1-fmt.log` | `E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855` |
| `shortcuts-stage1-build.log` | `C9CE92E4A3CD8D44A5ECBB58D2625E29484DE2B97D019137E5EF48B5CF3F4F20` |
| `assets/keyboard-shortcuts-light.png` | `6F93EC41A50D151FF8646899565C4CD9CD1F30CBF58D91DC6E01C742D32B2608` |
| `assets/keyboard-shortcuts-dark.png` | `A1D4DBB7EC660B04F7D1A82D5A187A428A46CEC5469EAC25A4CEF0665FF5BC2C` |
| `shortcut-02-targeted-final.log` | `9A0F60783806E7341D08666C1016C423F6F43F134313EE9225343F8A4CFF43FA` |
| `shortcut-02-plugin.log` | `79078AF81333A7893C2BC5F8E8F37746EFE2F9130254CB753F2F198FA868BCF1` |
| `shortcut-02-workspace.log` | `FDECEB30AC0C878772ACB3C976BE525277A60BAB16326B20A00E354DFC4E8E4E` |
| `shortcut-02-check.log` | `1157AAC24DEC7A36BE0FFAE7928A593B515871FC0CE6DC68CFABC391FD55F07B` |
| `shortcut-02-fmt.log` | `E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855` |
| `shortcut-02-build.log` | `DF5B63F4C011FAA8F510F2A2B1EAD139D50F4E938E555A07304471AD775E969A` |
| `assets/keyboard-shortcuts-binding-reloaded.png` | `23CDF598AD5F3A5AECC7C392E180E1BD8AE1EAE5DB850DC037E77B1CF9633C10` |
| `assets/keyboard-shortcuts-inline-edit.png` | `0ABB51B7A50B575AA787288BD12CA37DFBB9EA4E44CC45D82A7E9D0803A84586` |
| `shortcut-03-lifecycle-red.log` | `69B54696CE03948894C2C7385BDBB9F8A9B73965CE0C2EE5145044C732EF8704` |
| `shortcut-03-targeted-final.log` | `BBE82519E126FF4E8D0937B74E9CEC66A5B8E00EA8F6A40ECB98C9DD88B5ECE8` |
| `shortcut-03-real-plugin.log` | `A9A376D35061D205938B8764D28EA1A0351DE9D3F29C9EA0FB417C4E2F113DD7` |
| `shortcut-03-worker.log` | `A48715D144FF2A7ECF3525631BBCDF33747014191A64F4057C7AF2342DEF2B36` |
| `shortcut-03-host-command.log` | `E57F1C496AE3A27AF617E0D100D0A6DA2EA34814D85146E05D3E82AD7C4ACEF8` |
| `shortcut-03-workspace.log` | `99D0DBE3BA5C10BE74E79CBE1C7C8FFFEB6DDDD013C83DAA858A2628F377D45F` |
| `shortcut-03-check.log` | `E5E642DBC1F5F67ADE7265B99C2C767B13C395160AB7EFEAD91305D3D02520F1` |
| `shortcut-03-fmt.log` | `E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855` |
| `shortcut-03-build.log` | `81A6D11D889198C83AF5F00FE132F8CFA9893591041049F676A2F29F592FF8C0` |
| `shortcut-03-app-full.log` | `D529986BCC47A918AF4CB0B73CED829145229CEC1025150BBFC8F8AFA904339E` |
| `shortcut-03-app-full-serial.log` | `C66656EBACD354E45A60BF397E6B55ECAE021553F8AE6FA7E1F687809457775F` |
| `shortcut-03-app-full-final.log` | `BBC7D27EBA33A4FEBED96F3E7B71E5BAF448B84C214CFAFC579DD728231A4B16` |
| `shortcut-03-native-preparation.log` | `113ED277BFC1B49FB21D77F0460330D9CF1855FF8F02CF4FB0031BFACF1EF3B4` |
| `shortcut-03-website.log` | `9CF208720153EA4AED6417874E56F85B1B6642B9DC13E302EDEE41AEE7BE8E31` |
| `assets/keyboard-shortcuts-restored-conflict.png` | `2AE0294393107CD4970082B09A9A91E640859745FFF90D20D3C89B05A7E4D717` |
| `assets/keyboard-shortcuts-disabled-plugin.png` | `A75C86BC9759CE80CDBC520F43EFAA84D481396308BED03D8019309130A3D58B` |
| `assets/keyboard-shortcuts-plugin-reloaded.png` | `123A3366240384E8015E51EC854C7BE54C7FBC0CFCF0D906ED73D8A88959EE0A` |

日志存在编译与链接警告，包括 Wasmtime/Tree-sitter 链接警告；针对性测试通过不表示构建无警告。

## 未全量执行的范围与交付核对

- 本任务相关 9 项真实 WASM 测试已显式执行；其他 ignored 场景没有全量执行，也不将其报告为通过。本次未修改 SDK 分发契约，不重复运行 SDK 分发矩阵。
- 原生没有修改 Windows 显示缩放设置，1.5 倍缩放与小窗口由 GPUI 应用测试验证；两步录入的原生超时尝试没有算作成功。
- 站点为纯中英文内容修改，`npm test` 的两个搜索测试因没有构建索引跳过；没有发布站点或执行搜索索引变更。
- 三单均已普通提交、推送并核对关闭。03 实现提交为 `9d91893e0bdc48b3617cf5e36c6e807c73546552`；关闭 #83 前核对 `origin/main` 与该提交相同，再读回 #83 为 `closed` / `completed`。本记录的后续纯文档提交只同步实际交付状态，不改变已验证的实现。

## 文档迁入新工作区

本次只迁入快捷键规格、选定的 C 图、三张实施工单及其发布记录。新工作区仍使用现有基线文档位置；本任务链接已适配，不搬迁其他主题或原工作区无关改动。

C 图源文件与新工作区副本的 SHA-256 均为 `FCFFB5BE8C8F86E45F6962F5ABFA392221E0C6B7A3ED9599741D2C1DE5531621`。该图是设计原型，不是原生实现截图。

返回[原生 UI 文档](../README.md)。
