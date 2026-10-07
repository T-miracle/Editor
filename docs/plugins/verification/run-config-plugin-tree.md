# 运行配置插件模板与配置树：分阶段验收

规格：[run-config-plugin-tree-v1](../specs/run-config-plugin-tree.md)。本记录覆盖 `Editor-run-debug-build` 分支的四阶段交付；后续另行授权的主分支集成见 [集成记录](run-config-main-merge-2026-10-07.md)，不能将本分支的历史证据直接当作合并后的验证。

## 01 / GitHub #68

交付提交：`af1e0c73131aceecfc7e3376a4969d03052391fd`；已核对 origin 分支一致，并读回 #68 为 closed / completed。

本阶段交付公开 `run.configurations 1.0`、有界异步调用、独立插件模板与原生表单、本机存储、应用／保存／取消及实际执行的最小闭环。沿用此前已确认的原生父子弹窗、选择器和本地按钮外观，必要的未提交前置实现随本切片纳入。

两个独立包 `configuration-alpha`、`configuration-beta` 由公共 SDK 在仓库外构建，再通过公开管理器安装；它们分别提供纵向、横向原生布局。夹具不是宿主白名单，真实 Rust 与终端插件由 04 接入。

| 场景 | 可观察证据 |
| --- | --- |
| C01、C02 | 原生 owned dialog、空选择提示、重开跟随外部配置，不自动选择另一条 |
| C04、C05、C08 | 添加抽屉尺寸与右栏位置；分组模板、默认值、同模板独立身份，添加不写盘／执行 |
| C06、C07 | 两包经公开服务编辑；保存重开恢复；真实进程收到含空格、引号、中文、元字符的原参数数组 |
| C27 | 规范化工作区键的本机写盘与重读；项目未产生共享配置文件 |
| 最低失败保护 | Apply 后 Cancel 保留已应用数据；无效保存禁止执行；写盘失败保留原生窗口与草稿 |

原生批次：`cargo test -p editor-app plugin_configuration -- --ignored`。前置：`cargo build -p editor-app`、`./scripts/build-configuration-example.ps1` 和已有实际终端包。SDK 分发：`./scripts/verify-plugin-sdk.ps1`；读者文档：在 `website/` 执行 `node --test tests/doc-sources.test.mjs`。

实现过程中发现并修复：双栏 flex 的垂直居中导致内容滚动区零高度；用空串模拟键入未删除原文本的测试误判；夹具原始路径与应用规范化存储键不一致；测试泵把尚未完成的公开执行查询当作未知状态。通过物理点击、键入、实际包和真实进程验证修复；未开放私有 Reference 或新增测试专属宿主 API。

2026-10-07 最终提交候选通过检查。候选通过 `git checkout-index` 导出到隔离目录 `target/run-config-plugin-tree/candidate-01`；实际 ZIP 夹具来自同一已纳入候选的源码与 SDK。检查日志保存在本机 `target/run-config-plugin-tree/`，不提交生成产物：

- `cargo fmt --check`、`cargo test --workspace --exclude editor-app`、`cargo check --workspace`：退出 0；非 UI 测试中的 ignored 项不计为通过。
- `cargo test -p editor-app -- configuration_icons selector sdk_export`：9 passed，0 failed。
- `cargo test -p editor-app plugin_configuration -- --ignored`：3 passed，0 failed，未跳过这三项实际包验收。
- `cargo build -p editor-app`、`./scripts/build-configuration-example.ps1`、`./scripts/verify-plugin-sdk.ps1`：退出 0，独立构建与 SDK 导出／修复通过。
- `website/` 的 `node --test tests/doc-sources.test.mjs`：9 passed，0 failed。
- `git diff --cached --check`：通过。

02–04 的目录树、完整事务、故障边界及实际插件交付仍待实施。原生输入法候选窗口没有在本阶段被宣称通过。

## 02 / GitHub #69

交付提交：`993fc1c00e60c1d7889b666acb68b87e7717b21b`；已核对 origin 分支一致，并读回 #69 为 closed / completed。

已实现 C03、C11–C19、C22。公开 SDK 的 `FormEvent::Rename` 使复制后的名称由插件更新自己的业务值，宿主不猜测表单节点或 JSON 字段。文件夹、父级与同类别顺序进入同一工作区存储；应用只合并当前配置及完整必要路径，不改变外部选择。

共用实际包与原生驱动新增 3 个成组场景：多配置编辑／应用／独立复制／取消／全部保存；原生鼠标拖动、根目录移动、同类别排序与循环拒绝；计数删除以及 X／Esc 的保存、放弃、继续路径。核心增加 3 个存储与树事务行为用例。生产字段与 Reference 均未为测试开放，仅共享已有的测试包驱动。

原生批次发现并修复内部虚拟列表零高度、拖动时被默认文件夹点击折叠，以及确认控件退场后 Esc 焦点失去接收路径的问题；异步加载后输入的夹具顺序也已修正。

2026-10-07 最终内容通过 `cargo fmt --check`、`cargo test --workspace --exclude editor-app`、`cargo check --workspace`；应用常规批次 `cargo test -p editor-app --bin editor-app -- configuration_icons selector sdk_export` 为 9 passed；实际包批次 `cargo test -p editor-app --bin editor-app plugin_configuration -- --ignored` 为 6 passed，0 failed。公开 SDK 分发及独立包重新构建通过；fixture 包版本提升至 0.1.1。文档源检查 9 passed，提交差异检查通过。

代码检查对应最终完整提交内容；所有相关源码与文档纳入 index，范围外 `Cargo.lock` 的 Git 规范化内容哈希与 HEAD 一致。没有借用后续工单代码或未纳入的生产源码使本阶段通过。03、04 尚待实施。

## 03 / GitHub #70

交付提交：`f6d4067a87f2ef2f646e5da396b4d50645841f44`；已核对 origin 分支一致，并读回 #70 为 closed / completed。

已实现并验证 C20、C21、C23–C26。失败或无法校验的配置保留原值，配置树、外部列表和已选名称标红并显示原因；构建、运行、调试分别重新校验，停止既有会话不依赖配置有效性。

原生输入事件在插件确认前保留为有界的、不透明的待确认日志；表单服务失败时仍可保存这些输入，重开后按原顺序恢复。不能用旧规范值校验尚未确认的新输入，不能在配额耗尽后把未保存的输入宣称已保存。状态、事件和事务各只有一份真相来源。

公开管理器提供只读的提供者实例与工作区来源凭据。响应在 UI 接收时和工作线程实际执行前分别检查；校验凭据不授予权限。拒绝启动也结束对应的准备状态，使后续修复或重试可用。窗口关闭只撤销本窗口的表单、目录与校验请求，不停止其他已拥有的会话。

独立夹具升级至 0.1.2，新增通过公开工作区文件能力触发的业务拒绝、错误响应、格式错误和真实 30 秒服务超时。新增三个成组原生场景覆盖输入恢复、各执行入口、响应与工作线程之间的退休竞态、工作区切换、配置删除、受限工作区、已有进程停止以及权限缺失。实际权限缺失按仓库安装规则拒绝安装，不修改私有授权字段。

开发批次先暴露了夹具缺少 `workspace.files` 声明，以及启动拒绝后准备状态未结束的产品缺陷，均已修复。真实调试器与 Windows IME 候选窗口留在 04 验收，不把模拟故障记作真实调试通过。

2026-10-07 最终检查通过，日志保存在本机 `target/run-config-plugin-tree/03-*.log`：

- `cargo fmt --check`、`cargo test --workspace --exclude editor-app`、`cargo check --workspace`：退出 0；非 UI ignored 项不计为通过。
- `cargo test -p editor-app --bin editor-app plugin_configuration -- --ignored --nocapture --test-threads=1`：9 passed，0 failed；实际故障组包含真实等待预算，共用包和树事务场景也全部通过。
- `cargo test -p editor-app --bin editor-app -- configuration_icons selector sdk_export menu::tests sequence::tests`：30 passed，0 failed。
- `cargo build -p editor-app`、`./scripts/verify-plugin-sdk.ps1`：退出 0，SDK 导出、修复及仓库外构建通过；0.1.2 独立配置包已由本阶段 SDK 构建并用于原生批次。
- `website/` 的 `node --test tests/doc-sources.test.mjs`：9 passed，0 failed。SDK 中英文规范保持一致。
- `git diff --cached --check`：通过。审查范围只包含本阶段输入保全、来源检查、状态和门禁及必要的消费者与文档，不混入其他工作区修改。

## 04 / GitHub #71

实际接入 Rust 0.5.0 和终端 0.12.0，完成新窗口生产入口切换。公开 `process 1.6` 提供不启动程序的工具解析，`ui.native 1.1` 提供原生多行输入；可选访客 `command_form` 只组合普通字段，命令策略仍由插件决定。Rust 提供 Cargo/run、build、debug 模板；子命令和选项均为可编辑参数。终端按宿主 OS 和实际可解析解释器筛选模板，不展示其他系统或缺失解释器。

两个消费者与独立配置包均通过公开 SDK 构建。Rust 包换为 `renamed-cargo` 身份后仍通过原生窗口、插件校验、Cargo JSON 产物准备和真实 CodeLLDB 路径；宿主不按 Rust 或终端身份执行特殊分支。`$self` 只绑定提供者自己的 provided target，包括匹配的构建准备步骤，不修改不透明业务值或授予权限。

原生 Windows 验收使用独立目录 `target/run-config-plugin-tree/native-04` 和专用插件 runtime，未更改主工作区或全局插件安装。80 条树记录、80 个参数用于长树与长表单检查；这是准备夹具，不能计为产品测试通过。实际观察到：空外部选择时右栏显示“请添加配置”；四个图标工具按钮、固定提交栏、分组添加抽屉和只读程序显示正确；树与表单可分别滚动到底部。浅色 14 px、深色 16 px 的真实窗口可编辑，操作栏保持可见。

Microsoft 拼音通过物理键盘输入实测，单行名称及多行脚本均出现真实系统候选窗口：`n` 进入预编辑，`i` 替换为 `ni`，空格提交“你”并清除候选／预编辑；异步插件响应没有追加旧组合文本或夺走焦点。多行脚本的 Enter 产生换行，没有提交整个弹窗。取消撤销所有未应用验收草稿。中英文与完整 10–24 px 字号端点由实际包 GPUI 成组用例补充覆盖，不将模拟输入记作系统 IME 证据。

实际 Windows 发现长下拉被挤到按钮上方、编辑入口必须滚到底部，以及滚动引发的静止鼠标 hover 覆盖 End 键选择。修正通用本地菜单后，最新 release 的 80 条下拉保持按钮正下方，分隔线及“编辑配置”固定可见；End/Enter 能正确打开新窗口。其他菜单默认行为通过共用回归验证。

旧数据清理先在隔离副本证明只删除指定工作区的旧本机／共享文件，重复清理为空；其他工作区、新插件配置、插件私有数据、安装记录和设置保留。实际授权目录仅为 `C:/Projects/RustProjects/Editor-run-debug-build`，规范化存储键对应 `84b8f53dc8807f6d.json`。预览、清理及重复执行均返回 `[]`，该目录没有旧配置待删，没有伪称删除用户文件。主工作区配置、新配置、安装记录、插件私有数据等 34 个受保护路径的 SHA-256／不存在状态均保持一致，详情只留本机 `04-cutover.json`。普通启动不自动清理，不导入旧配置。

开发批次发现并修复：Scroll 根节点未 grow 导致表单不可见；provided debug 目标缺少匹配准备步骤导致显示标签被送给调试器；保存关闭 HWND 后测试仍绘制已移除窗口；终端原有交互 Shell 在激活时启动，被夹具错误当作目录查询的副作用。最终用例比较激活后的已有进程基线，目录查询不新增配置执行。菜单测试的两个旧格式夹具已换成公开插件元数据格式，没有放宽执行门禁。

2026-10-07 最终交付检查通过。完整日志在本机 `target/run-config-plugin-tree/04-*.log`，生成包和日志不提交：

- `cargo fmt --check`、两个插件的 `cargo fmt --manifest-path ... --check`、`cargo check --workspace`：退出 0。
- `cargo test --workspace --exclude editor-app`：182 passed、160 ignored、0 failed；ignored 不计为通过。表单间距调整后追加 `cargo test -p plugin-protocol`：30 passed、0 failed，未重新运行不受影响的全部检查。
- `cargo test -p editor-app --bin editor-app -- configuration_icons selector sdk_export menu::tests sequence::tests`：31 passed、0 failed。
- 实际包统一批次 `cargo test -p editor-app --bin editor-app plugin_configuration -- --ignored --nocapture --test-threads=1`：13 passed、0 failed、0 ignored，699.86 秒；包含三组窗口、三组树事务、三组故障和四组实际插件验收，不包含准备夹具。
- `editor-app.exe --plugin-cargo plugins/terminal/Cargo.toml test`：44 passed、0 failed；原有终端行为与新增 OS 策略一并检查。
- `cargo build -p editor-app`、`./scripts/build-configuration-example.ps1`、`./scripts/build-plugins.ps1 -HostExe target/debug/editor-app.exe -Packages rust,terminal`、`./scripts/verify-plugin-sdk.ps1`：退出 0；SDK 导出、修复和仓库外独立构建通过。
- `website/` 的 `node --test tests/doc-sources.test.mjs`：9 passed、0 failed，中英消费者文档同步。
- `./scripts/package-editor.ps1 -Output dist/editor-run-config-tree`：退出 0，10 个实际插件包及 release 可执行文件生成。测试包与发行包中 Rust、终端 WASM 的 SHA-256 分别一致；版本与能力声明核对一致。
- release 可执行文件 SHA-256：`334b1bcca9d4d52fbab2791b46ea8c164f8432a53a9582a38662022f39dc4128`；新版下拉的最终 Windows 交互使用该文件。
- 已按仓库约定检查公开机制、插件策略边界、输入／资源生命周期、旧数据清理范围及最终提交范围。没有新增测试专用宿主 API，没有升级全局工具链或修改主工作区既有改动。提交差异检查通过，交付 SHA 与推送／关闭回读记录在 #71。

### 最终 C01–C30 证据索引

各行指向本规格的实际验收，历史 B3 结果没有补入。实际包测试入口统一为 `cargo test -p editor-app --bin editor-app plugin_configuration -- --ignored --nocapture --test-threads=1`；`native_configuration_tests`、`tree_tests`、`fault_tests`、`rollout_tests` 均是经公开管理器安装的真实 WASM 包加原生 GPUI 窗口。Windows 系统 IME 的单独证据见上文。

| 场景 | 本批证据 |
| --- | --- |
| C01 | `native_configuration_tests`：owned dialog；真实 Windows 父子窗口与关闭回读 |
| C02 | 同组保存重开；真实 Windows 有树而无外部选择时仍显示空提示 |
| C03 | `tree_tests`：树结构、图标工具栏及选择；Windows 图标、tooltip 与焦点检查 |
| C04 | 实际包添加抽屉尺寸、树覆盖范围和右栏保持；Windows 分组抽屉 |
| C05 | 两独立包目录；`rollout_tests` 的实际 Cargo/Shell 分组 |
| C06 | 两独立包不同原生布局；实际 Rust 只读 cargo、终端多行脚本 |
| C07 | 包编辑保存重开；真实 Cargo 含空格／中文／引号／元字符的 argv 和 Shell 脚本执行 |
| C08 | 添加不写盘／执行，同模板新身份；真实模板默认值 |
| C09 | `rollout_templates_filter_tools_without_execution`：Cargo.toml 缺失置灰并给原因 |
| C10 | 同组仅显示本机解析成功的解释器；终端访客 OS 策略用例，其他 OS 为策略检查 |
| C11 | `tree_tests`：文件夹内、配置同级及根目录三种新增位置 |
| C12 | 原生鼠标跨层／根目录拖动、自包含及后代循环拒绝；工作目录不变 |
| C13 | 同类拖动排序、文件夹优先及保存重读 |
| C14 | 独立复制、默认选中、改名、取消回退及文件夹不可复制 |
| C15 | 递归数量确认、草稿删除及取消／保存边界 |
| C16 | 多配置来回切换后各自输入保留 |
| C17 | 当前配置及必要父级路径单条 Apply，未夹带其他草稿，外部选择不变 |
| C18 | Apply 后继续编辑再 Cancel，已应用基线保留 |
| C19 | Save 全部校验、树／各配置写盘、外部选择和窗口关闭 |
| C20 | `fault_tests`：业务拒绝仍保存原值，树／外部文本标红，执行门禁 |
| C21 | 真实服务错误、格式错误、30 秒超时及待确认输入恢复 |
| C22 | X／Esc 的保存、放弃、继续三路和已应用内容保留 |
| C23 | Build／Run／Debug 各自重验环境；实际 Cargo/Shell 和真实调试链路 |
| C24 | 配置无效时已有会话仍可停止；真实调试暂停后停止与资源释放 |
| C25 | UI／actor 间退休竞态、配置删除、工作区切换后的迟到结果拒绝 |
| C26 | 受限工作区和缺权限拒绝，校验凭据不授予权限，不放开私有保护 |
| C27 | 本机按规范化工作区隔离重读，项目无新增共享配置或保存位置选项 |
| C28 | 核心隔离清理用例与实际工作区 preview／clear／repeat、受保护路径核对 |
| C29 | 真实 Windows 单／多行 IME、主题、字号、焦点和长列表滚动；实际包中英／主题／字号成组回归 |
| C30 | 独立 alpha／beta 和换身份 Rust；窗口关闭、工作区切换、提供者退休后的视图／订阅／请求撤销 |
