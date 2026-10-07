# 快捷键面板阶段验证记录

日期：2026-10-07

状态：工单 01 查询能力已通过应用测试、真实插件测试和原生验收；工单 02、03 仍在实施，整项功能尚未完成。

规格：[快捷键面板与用户绑定](../specs/keyboard-shortcuts.md)。工单：[01 → 02 → 03](../tickets/keyboard-shortcuts/README.md)，对应 [#81](https://github.com/T-miracle/Editor/issues/81) → [#82](https://github.com/T-miracle/Editor/issues/82) → [#83](https://github.com/T-miracle/Editor/issues/83)。父规格 #77 保持原样且不关闭。

## 工作树与证据范围

- 工作区：`C:/Projects/RustProjects/Editor-shortcuts-main`；用户要求使用 `main`，基线提交为 `ae63225afd6e8a362c6f186d7374323e97405469`。
- 下列结果来自该基线之上的快捷键实现中间工作树，尚无独立交付提交。各阶段结果只证明当时执行的场景，不代表随后继续修改的全部工作树已经复验。
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

日志存在编译与链接警告，包括 Wasmtime/Tree-sitter 链接警告；针对性测试通过不表示构建无警告。

## 尚未完成的验证

- 原生已验收工单 01 的查询交互；行内编辑、持久化和生命周期的原生组合验收留待后续实现。
- Ctrl+Backspace 的改绑与旧路径撤销由工单 02 补可观察的文档行为，不以目录去重替代执行结果。
- 工单 02 的行内修改、多绑定、恢复默认、冲突、实际分发、用户配置保存及重载和未保存保护。
- 工单 03 的插件停用/卸载/恢复、过期连续按键撤销、恢复冲突、受限工作区与最终组合验收。
- 后续改动须按影响复验；当前构建成功不代替原生交互验收。
- 本阶段真实 WASM 查询测试已单独通过；它不覆盖插件生命周期、全部 ignored 测试或 SDK 分发验证。
- 最终交付审查汇总、普通提交、推送核对与实施议题关闭尚未完成；本记录不改变任何工单状态。

## 文档迁入新工作区

本次只迁入快捷键规格、选定的 C 图、三张实施工单及其发布记录。新工作区仍使用现有基线文档位置；本任务链接已适配，不搬迁其他主题或原工作区无关改动。

C 图源文件与新工作区副本的 SHA-256 均为 `FCFFB5BE8C8F86E45F6962F5ABFA392221E0C6B7A3ED9599741D2C1DE5531621`。该图是设计原型，不是原生实现截图。

返回[原生 UI 文档](../README.md)。
