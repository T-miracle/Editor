# 快捷键面板阶段验证记录

日期：2026-10-07

状态：工单 01 已交付，提交 `495a1d0` 已推送且 #81 关闭状态已读回核对。工单 02 实现、自动验证及必要原生行为验收已完成，普通提交、推送及关闭核对待执行；工单 03 尚未完成，整项功能尚未完成。

规格：[快捷键面板与用户绑定](../specs/keyboard-shortcuts.md)。工单：[01 → 02 → 03](../tickets/keyboard-shortcuts/README.md)，对应 [#81](https://github.com/T-miracle/Editor/issues/81) → [#82](https://github.com/T-miracle/Editor/issues/82) → [#83](https://github.com/T-miracle/Editor/issues/83)。父规格 #77 保持原样且不关闭。

## 工作树与证据范围

- 工作区：`C:/Projects/RustProjects/Editor-shortcuts-main`；用户要求使用 `main`，基线提交为 `ae63225afd6e8a362c6f186d7374323e97405469`。
- 工单 01 对应交付提交 `495a1d0`；工单 02 的结果来自该提交之上的实现工作树，最终两轴审查以 `git diff --cached 495a1d0` 为范围，尚未填写 02 交付提交。各阶段结果只证明当时执行的场景，不代表随后继续修改的全部工作树已经复验。
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

工单 02 行为验收已满足，普通提交、推送与 #82 关闭读回核对尚待执行；不预填交付提交或关闭状态。

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

日志存在编译与链接警告，包括 Wasmtime/Tree-sitter 链接警告；针对性测试通过不表示构建无警告。

## 尚未完成的验证

- 工单 03 的插件停用/卸载/恢复、过期连续按键撤销、恢复冲突、受限工作区与最终组合验收。
- 后续改动须按影响复验；当前构建成功不代替原生交互验收。
- 真实 WASM 查询与静态有效插件编辑测试已单独通过；它们不覆盖插件生命周期、全部 ignored 测试或 SDK 分发验证。
- 工单 01 已提交、推送并核对 #81 关闭；工单 02、03 的普通提交、推送核对与实施议题关闭尚未完成。本记录不自行改变跟踪器状态。

## 文档迁入新工作区

本次只迁入快捷键规格、选定的 C 图、三张实施工单及其发布记录。新工作区仍使用现有基线文档位置；本任务链接已适配，不搬迁其他主题或原工作区无关改动。

C 图源文件与新工作区副本的 SHA-256 均为 `FCFFB5BE8C8F86E45F6962F5ABFA392221E0C6B7A3ED9599741D2C1DE5531621`。该图是设计原型，不是原生实现截图。

返回[原生 UI 文档](../README.md)。
