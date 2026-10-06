# 运行配置插件模板与配置树：分阶段验收

规格：[run-config-plugin-tree-v1](../specs/run-config-plugin-tree.md)。分支：`Editor-run-debug-build`；主工作区未合并。

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

待实施：C09、C10、C28–C30，最终场景矩阵和实际插件包验收。
