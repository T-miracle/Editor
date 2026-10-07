# 运行配置重构：插件模板、原生表单与配置树

日期：2026-10-07

稳定标识：`run-config-plugin-tree-v1`

状态：用户已确认产品验收基线、主要测试接缝及 4 张工单拆分，并要求依次实施。[规格 #67](https://github.com/T-miracle/Editor/issues/67) 与 #68–#71 已发布并读回确认；四张实施工单的实现、行为验收、普通提交与推送均完成，并已核对关闭，证据见 [分阶段验收](../verification/run-config-plugin-tree.md)。规格议题保留为设计依据，`ready-for-agent` 标签本身不代表完成。

最初实施限定于 `Editor-run-debug-build` 分支及对应工作区。2026-10-07 用户另行授权合并主分支并推送远程；集成采用隔离候选验收，并保留主工作区原有未提交修改，见 [主分支集成记录](../verification/run-config-main-merge-2026-10-07.md)。

## Problem Statement

现有运行配置窗口采用宿主统一表单和平面列表。用户无法按自己的项目结构组织配置，插件也无法完整呈现其命令需要的布局、参数和交互。发现目标、手动程序和脚本入口混合，用户难以分辨程序、子命令、参数以及插件提供的默认值。

保存行为也不适合同时编辑多条配置：单条草稿切换需要处置修改，删除曾立即持久化，外层取消无法撤销。用户需要明确区分“应用当前配置”“保存全部并关闭”和“放弃未保存修改”。用户还希望保留尚未配置正确的内容，以便稍后修复，同时保证这些配置不能被执行。

这次重构需要同时明确插件边界、配置树、草稿与持久化、校验及执行门禁。仅改变控件排列不能满足需求。

## Solution

保留与插件管理相同的独立原生配置窗口，采用左右两栏。左栏顶部提供添加、删除、复制、添加文件夹四个图标按钮，主体使用资源管理器式配置树。添加按钮打开覆盖左栏目录树的等宽抽屉，按插件提供的分组展示“图标＋命令模板名称”。选择模板后创建未保存配置，关闭抽屉并选中新配置。

右栏由所选配置的插件自定义原生布局和交互，包含配置名称、只读程序、可编辑参数等内容。插件提供模板、默认值、适用性判断及业务校验；宿主提供公共机制、统一基础控件外观、目录组织、保存协调和执行门禁。插件不自行决定窗口级保存、应用和取消行为。

所有配置及配置树按工作区保存在本机。应用只保存当前配置及必要文件夹路径，保存提交全部并关闭，取消只放弃尚未保存的修改。校验失败也允许保存，但配置名称标红并说明原因，构建、运行、调试均不可执行。每次执行前重新调用插件校验，停止已有会话仍保持可用。

### 领域用语

| 术语 | 本规格中的含义 |
| --- | --- |
| 命令模板 | 插件提供的可重复实例化定义，包含分组、图标、名称、默认数据及关联表单与校验能力；不是已保存配置，也不是正在执行的会话 |
| 模板分组 | 添加抽屉中的分类，例如 Cargo；与用户创建的配置文件夹不同 |
| 程序 | 受控启动的可执行程序，例如 cargo；配置表单中只读，由插件确定 |
| 参数 | 传给程序的完整参数，包含适用的子命令和选项；插件预填，用户可修改，运行时保留结构化边界 |
| 运行配置 | 从模板创建并由用户编辑的独立数据，具有稳定身份、名称、所属工作区、提供者关联及插件配置值 |
| 配置文件夹 | 用户创建的虚拟组织节点，可嵌套；不对应磁盘目录，不改变运行工作目录 |
| 窗口草稿 | 当前窗口内尚未提交的配置和目录树修改；切换节点不能丢失这些修改 |
| 已应用基线 | 本次应用或保存成功后已经持久化的内容；取消不能撤回已应用内容 |
| 校验结果 | 插件针对指定配置快照给出的通过或失败结果，或宿主报告的无法校验状态；不等于执行授权 |
| 执行会话 | 既有构建、运行或调试会话，仍遵循原有提供者、权限和资源生命周期契约 |

## User Stories

1. As an editor user, I want the configuration window to behave like plugin management, so that native window ownership and focus remain familiar.
2. As an editor user, I want a two-column configuration window, so that organization and editing remain visible together.
3. As an editor user, I want Add, Delete, Copy and Add Folder as icon buttons, so that the left toolbar stays compact.
4. As an editor user, I want tooltips and accessible names for icon buttons, so that each action remains understandable without a visible label.
5. As an editor user, I want configurations shown in an explorer-style tree, so that I can organize a large collection.
6. As an editor user, I want virtual folders independent of disk directories, so that organizing configurations does not change execution paths.
7. As an editor user, I want nested configuration folders, so that project-specific organization is possible.
8. As an editor user, I want to move configurations and folders by dragging, so that I can reorganize existing entries.
9. As an editor user, I want invalid self-descendant folder moves rejected, so that the tree remains usable.
10. As an editor user, I want folders listed before configurations, so that the hierarchy is easy to scan.
11. As an editor user, I want manual order within each node category, so that frequently used entries can stay near the top.
12. As an editor user, I want saved tree order restored, so that reopening does not rearrange my work.
13. As an editor user, I want new entries placed relative to my selection, so that I do not need an extra move after every addition.
14. As an editor user, I want an affected-item count before deleting a folder, so that recursive deletion is intentional.
15. As an editor user, I want deletion staged until Save, so that Cancel can restore unapplied removals.
16. As an editor user, I want to copy a configuration into the same folder and select its named copy, so that I can quickly create a variant.
17. As an editor user, I want Copy disabled for folders, so that the supported scope is clear.
18. As an editor user, I want the Add drawer to cover only the left tree area, so that the right panel does not shift.
19. As an editor user, I want the toolbar to remain visible while the drawer is open, so that the window structure remains stable.
20. As an editor user, I want grouped templates with icons and names, so that commands from different tools are easy to find.
21. As an editor user, I want a template selection to create and select a draft, so that I can immediately edit its defaults.
22. As an editor user, I want multiple configurations created from one template, so that different argument sets can coexist.
23. As an editor user, I want templates supplied by enabled workspace plugins, so that installed integrations determine available configuration types.
24. As an editor user, I want inapplicable ordinary templates disabled with reasons, so that absence of support is distinguishable from temporary unavailability.
25. As an editor user, I want only locally available Shell templates shown, so that irrelevant platforms and missing interpreters do not clutter the drawer.
26. As a terminal plugin developer, I want to detect available interpreters and provide their templates, so that Shell policy stays outside the host.
27. As an editor user, I want the program displayed read-only and arguments editable, so that I can adjust a template without confusing the executable with its parameters.
28. As an editor user, I want default subcommands to remain editable as arguments, so that a Cargo template does not lock part of my argument list.
29. As an editor user, I want each plugin to provide its own native form layout, so that specialized command settings are represented naturally.
30. As a plugin developer, I want public native UI composition and events for configuration panels, so that I can implement layouts without host-specific branches.
31. As an editor user, I want consistent base control appearance and native input behavior across plugin forms, so that customization does not undermine usability.
32. As an editor user, I want the initial selection to match the external run selector, so that opening the window edits the intended configuration.
33. As an editor user, I want no automatic selection when the external selector has none, so that an unrelated configuration is not chosen silently.
34. As an editor user, I want an empty panel with an invitation to add a configuration, so that the first action is clear.
35. As an editor user, I want node switching to retain unsaved values, so that I can edit several configurations in one visit.
36. As an editor user, I want Apply to save only the selected configuration, so that I can commit one change without submitting all drafts.
37. As an editor user, I want Apply to include necessary parent folders, so that the saved configuration stays in its intended location.
38. As an editor user, I want Apply to keep the window open and preserve the external selection, so that saving an edit does not change my next run target.
39. As an editor user, I want Save to persist all configurations and tree changes and close the window, so that one action completes the editing session.
40. As an editor user, I want Save to select the edited configuration externally, so that the completed selection becomes my next run target.
41. As an editor user, I want Cancel to discard only unsaved changes, so that already applied work is retained.
42. As an editor user, I want a Save, Discard or Continue Editing prompt when closing a dirty window, so that an accidental close does not lose work.
43. As an editor user, I want every configuration checked before Save, so that stored validity reflects plugin feedback.
44. As an editor user, I want invalid configurations saved with their entered values, so that unfinished work is not lost.
45. As an editor user, I want invalid configuration names shown in red with reasons, so that problems remain visible after saving.
46. As an editor user, I want configurations retained when a plugin cannot validate them, so that plugin faults do not destroy my edits.
47. As a plugin developer, I want to own command-specific validation rules, so that the host does not reproduce tool policy.
48. As an editor user, I want Build, Run and Debug disabled for invalid or uncheckable configurations, so that incomplete settings cannot execute.
49. As an editor user, I want Stop to remain available for existing sessions, so that invalid configuration edits do not prevent cleanup.
50. As an editor user, I want fresh plugin validation before every execution attempt, so that changed tools and project state are detected.
51. As an editor user, I want stale validation results rejected, so that an older response cannot authorize a newer configuration.
52. As an editor user, I want configurations and tree state stored locally per workspace, so that configuration editing does not write project files.
53. As an editor user, I want the local/shared selector removed, so that this version presents one clear storage model.
54. As a project maintainer, I want the authorized legacy cleanup limited to the reconstruction workspace, so that other workspaces and plugin data are untouched.
55. As a plugin developer, I want template, form and validation capabilities available to all compatible plugins, so that adding an integration does not require host modifications.
56. As an editor user, I want workspace trust and existing permissions enforced independently of validation, so that a passing result cannot grant execution authority.
57. As an editor user, I want Chinese and English text, both themes, keyboard navigation, IME, scrolling and scaling supported, so that the new interface remains a native part of the editor.
58. As a project maintainer, I want acceptance driven through actual plugin packages and the native host, so that passing tests demonstrate public behavior rather than private implementation details.

## Implementation Decisions

### 1. 本规格与既有决定的关系

本规格是运行、调试与构建能力之上的配置管理重构。以下已确认变更替代旧配置界面约定；未涉及的执行、调试、受控进程、工作区信任与资源生命周期语义继续有效。

| 旧约定 | 本轮已确认替代 |
| --- | --- |
| 宿主统一字段与折叠区组织所有配置表单 | 插件自定义右栏原生布局和交互，宿主统一基础控件外观及窗口级提交操作 |
| 左栏平面列表、发现按钮及更多菜单 | 资源管理器式配置树与添加、删除、复制、添加文件夹四个图标按钮 |
| 宿主提供手动程序及通用 Shell 入口 | 只使用插件命令模板；Shell 模板由终端插件按本机环境提供 |
| 单配置草稿，删除确认后立即持久化 | 窗口管理多配置及树草稿；删除也需保存，取消可撤销尚未保存的删除 |
| 仅本机／项目共享切换 | 移除选择器，本版统一按工作区保存在本机 |
| 校验失败阻止保存 | 保存前仍须校验，但失败或无法校验的内容也保存，标红并阻止执行 |
| 旧配置需要迁移 | 本次仅对指定重构工作区一次性清理旧运行配置；不建立普遍自动删除策略 |

之前讨论过的共享文件夹路径随项目传播随“仅本机”决定撤销，不纳入实施。原有独立原生窗口机制、紧凑图标风格、外部配置下拉的按钮下方定位及“配置列表／分割线／编辑配置”结构继续保留。本规格不修改或关闭历史父设计议题，不以新需求否定旧阶段实际验证记录。

### 2. 窗口、选择与右栏

- 复用与插件管理相同的原生窗口机制，保持在所属主窗口上方；这里的置顶不是覆盖其他应用的系统全局置顶。
- 左右两栏，左栏工具栏与配置树，右栏插件表单；使用项目本地 UI 层和原生行为基座，不引入 WebView。
- 打开时以外部运行按钮组的有效配置选择为准；没有有效选择则不自动选中另一条配置，右栏空态居中显示“请添加配置”。
- 插件控制自身表单布局和事件，包含配置名称、只读程序、参数及其需要的设置。宿主不将其限制为同一套固定字段排列。
- 插件布局继续遵循宿主原生 UI、主题、权限、焦点和资源规则，不允许以自定义面板绕过统一的应用、保存、取消边界。
- 在树中切换配置仅切换当前编辑对象，保留其他草稿；不立即切换外部运行目标。
- 应用不切换外部选择；保存成功关闭时，外部按钮组同步当前选中的配置。不存在有效配置时不得留下已删除或失效的配置引用，不隐式启动其他目标。

### 3. 配置树与结构操作

- 文件夹是虚拟分组，允许多层嵌套，独立于磁盘路径和程序工作目录。
- 支持拖拽配置与文件夹到其他文件夹或根目录；禁止把文件夹放入自身或后代。
- 同一父节点下文件夹在前、配置在后；各类别内支持手动拖拽排序。新增项追加到所属类别末尾，保存后重开保留顺序。
- 添加配置和文件夹时：选中文件夹则放入其中；选中配置则放在其同级；无选择则放在根目录。
- 复制仅作用于配置；在同级创建独立副本，名称自动带“副本”含义，选中新副本。选中文件夹时禁用复制，不递归复制文件夹。
- 删除文件夹包含其全部后代；确认前展示受影响的文件夹和配置数量，确认后只修改草稿。取消窗口可恢复尚未保存的删除。
- 结构操作不得改变已运行会话持有的启动快照；已有会话的停止与清理仍由既有会话机制负责。

### 4. 添加抽屉、分组与环境筛选

- 抽屉与左栏等宽，覆盖左栏目录树区域并保留顶部工具栏；不推移右侧表单。
- 插件提供分组与模板项。每项显示图标和命令模板名称；模板分组不是用户配置文件夹，不向配置树自动复制整个模板目录。
- Cargo 是分组示例，run、build、debug 是可选模板示例。同一模板可重复创建不同配置，模板身份与配置身份不能混为一体。
- 选择模板后，以插件默认值创建草稿、关闭抽屉并选中新配置；添加与选择不隐式运行、构建或调试。
- 模板来自当前工作区已启用的插件。普通模板不适用时置灰并说明原因，不能因为安装顺序改变有效提供者选择。
- Shell 使用用户特别确认的筛选规则：终端插件识别操作系统及本机解释器可用性，只展示实际检测到且可用的模板；其他系统或未安装的 Shell 隐藏，不套用普通模板的置灰规则。
- 环境判断和模板可用性属于插件策略；宿主按通用模板结果展示，不按终端插件、语言或程序名称编写专属判断。

### 5. 程序、参数与插件表单

- 程序由插件提供并在表单只读显示；参数完整可编辑，包括插件预填的子命令和选项。
- 对 cargo run --release 的示例，程序是 cargo，参数包含 run 与 --release。不得把“run 模板”解释为 run 子命令在参数中不可修改。
- debug 是模板显示示例，不意味着存在名为 cargo debug 的固定命令；实际配置数据及调试行为由插件通过已有通用执行／调试能力表达。
- 插件拥有配置名称、参数和专属设置的业务规则及错误说明；宿主负责工作区、实例、数据归属、协议有效性、存储和授权等通用约束。
- 保留参数数组及显式脚本语义；不得在宿主中拼接用户输入为 Shell 命令，或用程序名推断参数规则。
- 插件表单必须能够与宿主窗口草稿及持久化协调，不另设绕过窗口级应用、保存、取消的配置提交路径。

### 6. 应用、保存与取消

| 操作 | 校验与持久化范围 | 窗口与选择 |
| --- | --- | --- |
| 应用 | 调用插件校验当前配置，保存当前配置及维持其位置所必需的文件夹路径；不提交其他配置和无关树草稿 | 窗口保持打开，外部选择不变 |
| 保存 | 先校验全部配置，等待每条产生通过、失败或无法校验的结果，再保存全部配置与目录树修改 | 保存成功关闭窗口，并同步当前配置到外部选择 |
| 取消 | 放弃尚未保存的配置及树修改，保留已经应用的内容 | 关闭窗口，不执行目标 |
| 关闭按钮／Esc 关闭窗口 | 有未保存修改时提示保存、放弃修改、继续编辑；分别遵循上述保存、取消或保留草稿语义 | 已应用基线始终保留 |

- 当前配置位于尚未持久化的新文件夹时，应用也保存必要父级路径，不能临时把配置挪到根目录。
- 应用后继续修改，再取消时恢复到已应用基线，而不是恢复到打开窗口前的一切内容。
- “校验失败也保存”不等于忽略实际写盘失败。存储失败必须保留待处理内容并报告，不能宣称保存成功或因此启动程序。
- 应用和保存本身不启动配置目标、构建或调试会话。插件的必要环境探测仍受既有公开能力、权限及信任限制。

### 7. 校验状态与执行门禁

- 业务校验规则由插件提供；宿主调用并展示结果，不复制 Cargo、Shell 或其他具体工具的规则。
- 保存前校验全部配置，应用前校验当前配置。失败仍提交用户内容；不能恢复为旧版“任一配置错误就阻止全部保存”。
- 插件缺失、不可用、异常或有界等待超时均表示无法校验，不得当作通过。允许保存，配置名称标红并展示原因。
- 失败和无法校验都禁止构建、运行、调试；已有会话的停止操作不因配置无效而被禁用。
- 每次构建、运行、调试前重新校验指定配置，不能直接依赖上次保存的通过状态。失败则更新可见状态并阻止这次执行。
- 校验结果须绑定其工作区、配置身份、数据快照及提供者实例；配置或来源变化后，迟到结果不得覆盖新状态或批准新请求。
- 插件校验通过只是必要条件，不能替代执行能力、工作区信任、安装权限、资源归属及原有启动前检查。

### 8. 模块与公开契约

- 配置核心承接树、稳定身份、排序、窗口草稿、提交范围与本机持久化；应用集成承接窗口、抽屉、选择同步和原生交互。
- 协议与 SDK 承接模板贡献、分组与图标、插件配置面板、配置值交互、业务校验及结果语义；运行时继续承接能力协商、调用路由、权限、实例和资源撤销。
- 插件负责模板、环境筛选、布局、默认值、参数语义及业务校验。新增、替换或移除兼容插件，不应要求修改宿主业务代码。
- 所有新增宿主 API 必须是面向全部插件的公开、可复用通用能力。优先组合既有能力，不为 Rust、终端或某个模板创建独占接口或测试专用 API。
- 现有 run.targets 只提供目标发现和准备，不能据此宣称模板目录、图标、任意配置表单和配置校验已经受支持。原生 UI 协议可复用，但运行配置面板与草稿提交的衔接仍需要定义。
- 复用既有请求关联、取消、超时和资源归属机制。配置面板、草稿回调及校验请求在关闭窗口、切换工作区或提供者退休后须正确撤销，迟到事件不得作用到其他配置。
- 契约名称、版本号、具体字段、数据编码及默认预算属于实施时需明确的技术细节，本规格不虚构已存在的新接口。公开变化须同步 SDK、消费者、文档与包声明。
- 复用既有模块，不新增仅转发调用的包装层，不按逻辑层机械增加 crate；可见控件仍使用 gpui-base 行为及本地项目外观。

### 9. 存储、范围与一次性清理

- 配置及树结构统一按工作区保存在本机，不写入项目目录，移除“仅本机／项目共享”选择器。
- 旧配置不迁移。用户授权的删除仅为本次重构工作区的一次性旧运行配置清理，不是新版本打开任意工作区就清空数据的产品策略。
- 清理不扩展到主工作区、其他工作区、新格式配置、插件安装记录、插件私有数据、项目源码或全局设置。实施时先识别准确的旧运行配置存储范围，不能把“全部删除”解释为递归清空工作区或插件目录。
- 本次规格整理不执行数据清理、代码实现、提交、推送或合并；进入实施时仍在指定分支及工作区完成，保留已有无关改动。

## Testing Decisions

### 已确认的主要接缝

优先复用一个主要产品入口：**独立插件包 → 公开 Package／Manager → 原生配置窗口 → 本机持久化与既有执行会话入口**。

从真正可安装的插件包提供模板、图标、不同布局、默认参数和校验规则，通过原生窗口完成添加、编辑、应用、保存、重开与执行，观察可见结果、实际持久化及是否产生受控执行。无需把私有字段开放给测试，也不新增按具体插件区分的替换接口。

用户于 2026-10-07 确认沿用这一包管理器与宿主集成边界，并要求按已批准工单依次实施；确认测试入口不等于新功能已经通过验证。

### 测试原则与既有先例

- 只断言外部可观察行为、持久化结果、执行与资源边界，不断言私有容器形状、辅助函数调用次数或固定实现枚举。
- 复用真实独立目标包与 Rust 包经公开管理器进入原生宿主的发现、草稿保存及受控执行先例。增加不同身份、不同布局的独立模板提供者，证明宿主不认识具体插件名称仍可工作。
- 复用原生配置窗口生命周期、真实按钮键盘事件、字面参数保存、跨配置编辑及关闭回执的测试驱动方式。旧测试中“错误不落盘”“删除立即落盘”“项目共享”断言按本规格替换，不能同时保留相反产品行为。
- 配置核心的提交范围与树不变量可补就近测试；它们是同一产品行为的辅助验证，不另建一套绕过公共插件协议的模拟系统。
- 故障通过独立插件夹具和既有受控依赖边界注入。禁止用宿主插件 ID 特判、生产私有字段开放或修改用户真实插件数据来构造测试。
- 仅协议或 GPUI 测试平台通过不能替代真实 Windows 窗口、焦点、中文 IME 候选窗口与可见效果验收；跨平台 Shell 筛选夹具也不能被报告为已在所有操作系统真机通过。

### 行为验收矩阵

| 编号 | 场景 | 可观察的通过条件 |
| --- | --- | --- |
| C01 | 打开原生窗口 | 使用与插件管理相同的父子窗口机制、左右两栏；不引入 WebView，不宣称跨应用全局置顶 |
| C02 | 初始与外部选择 | 外部选中有效配置时打开对应表单；无有效选择时不选中其他配置，显示约定空态 |
| C03 | 左栏工具栏 | 四个图标按钮、悬浮提示及可访问名称正确，文字和图标对齐、留白一致 |
| C04 | 抽屉几何 | 与左栏等宽且覆盖树，顶部工具栏保留，右栏不因抽屉打开而移动 |
| C05 | 模板目录 | 独立插件提供的分组、图标和模板名称出现在抽屉；同一模板能创建多条独立配置 |
| C06 | 插件表单 | 两个独立提供者的不同原生布局均能编辑并往返保存，宿主无需具体插件分支 |
| C07 | 程序与参数 | 程序只读；预填子命令、选项均可编辑，空格、引号、中文和元字符不丢失参数边界 |
| C08 | 默认值与选择 | 选择模板创建草稿、抽屉关闭、新配置选中；未应用或保存前不落盘、不执行 |
| C09 | 普通模板可用性 | 已启用插件的适用模板可选，不适用模板置灰且说明原因 |
| C10 | Shell 特殊筛选 | 终端插件只提供本机可用解释器模板；其他系统及未安装解释器不显示，宿主无专属判断 |
| C11 | 新增位置 | 文件夹选中、配置选中、无选择三种情形分别放入、同级、根目录，追加位置正确 |
| C12 | 嵌套与移动 | 多层文件夹、跨父级及根目录移动有效；自包含和后代循环拒绝，不影响运行工作目录 |
| C13 | 排序持久化 | 文件夹在前，同类拖拽顺序可保存，关闭重开后保持 |
| C14 | 配置复制 | 同级出现独立副本并选中，内容可独立修改；取消可撤销未保存副本，文件夹不可复制 |
| C15 | 递归删除 | 确认展示影响数量；确认后仍是草稿，取消恢复、保存才持久化整个删除 |
| C16 | 多配置草稿 | 往返切换不同配置后，各自未保存字段和结构修改均保留 |
| C17 | 应用范围 | 只保存当前配置和必要父级路径；其他配置修改及无关树草稿不被夹带，外部选择不变 |
| C18 | 应用后取消 | 已应用内容保留，其后未保存修改和其他草稿撤销，重开内容与持久化基线一致 |
| C19 | 保存全部 | 全部配置经插件校验后一起保存，目录树及修改保留，窗口关闭，外部选择按约定同步 |
| C20 | 错误也保存 | 有校验失败配置时仍保存原值；名称标红、原因可见，三个执行入口禁用 |
| C21 | 无法校验 | 提供者不可用、异常或超时后仍可保存，不能误判通过或无限等待，配置标红并禁止执行 |
| C22 | 关闭草稿窗口 | 关闭按钮和 Esc 的保存、放弃、继续编辑三条路径正确，已应用内容不被撤回 |
| C23 | 每次执行重验 | 保存后修改工具或环境条件，下一次构建、运行、调试调用插件重新校验；失败时没有配置执行副作用 |
| C24 | 停止独立性 | 配置校验失败不会禁用已有会话的停止，原有会话归属及清理规则继续成立 |
| C25 | 异步隔离 | 旧配置快照、旧工作区或退休提供者的迟到结果不能覆盖新状态或启动目标 |
| C26 | 信任与权限 | 通过插件校验不等于授权；受限工作区、能力或权限不足仍拒绝执行，不通过配置授予权限 |
| C27 | 本机隔离存储 | 配置和树按工作区分别重开恢复；项目目录无新增配置文件，无共享选择器 |
| C28 | 清理边界 | 隔离副本证明只删除授权工作区的旧运行配置，不影响其他工作区、新配置或插件数据；不在测试中直接清理用户数据 |
| C29 | 原生交互 | 中英、深浅主题、键盘焦点、中文 IME、缩放及长树／长表单滚动可用，操作按钮不被裁掉 |
| C30 | 通用能力与退出 | 替换插件身份及模板无需修改宿主；关闭窗口、插件退出和工作区切换后相关面板与校验资源正确撤销 |

### 执行与证据要求

- 实施阶段先运行受影响模块测试，再按仓库要求执行 cargo fmt --check、cargo test --workspace --exclude editor-app、cargo check --workspace，以及相关应用测试和原生交互验收。
- 实际 WASM 包须先通过宿主公开 SDK 构建，再显式执行相关 ignored 用例；跳过不能计为通过。公开契约或 SDK 分发改变时，增加 SDK 分发验证及消费者包构建，并更新受影响插件版本和声明。
- 验收记录区分协议／状态测试、GPUI 测试平台、真实插件包和实际 Windows 交互；缺少工具链或平台时记录限制，不自动安装整个工具链或把模拟检查写为真机结论。
- 当前仅整理规格，检查链接、索引、术语、已确认决策一致性及 git diff --check，不运行无关 Rust 构建，也不把前一版测试结果作为本规格通过证据。

## Out of Scope

- 项目共享配置、共享目录树、保存位置切换和旧共享数据迁移。
- 宿主内置手动程序或 Shell 模板、宿主写死语言／工具业务规则，以及任一插件独占的宿主 API。
- 文件夹复制、真实磁盘文件夹管理、通过配置树移动改变程序工作目录。
- 针对所有工作区的旧数据清空策略、主工作区清理、插件私有数据或全局设置删除。
- 重新设计整个执行／调试系统、组合配置、跨项目工作流、附加／远程调试、自动安装编译器及其他未确认产品范围。
- 系统全局置顶、WebView 表单、插件自由改写全局主题或绕过宿主统一提交与权限机制。
- 合并主分支、修改或关闭历史父议题。用户已明确授权发布 4 张实施工单并依次执行；普通提交、推送与工单关闭遵循仓库既有交付授权，未验收完成不得关闭。

## Further Notes

### 依据与历史

- 本规格来源于 2026-10-07 的逐项确认及最后确认的 16 项验收基线；以最后决定为准，不恢复已撤销的“本机／共享”选项。
- [运行、调试与构建父议题 #48](https://github.com/T-miracle/Editor/issues/48)继续提供未被本规格替代的会话、执行、调试及权限基线。
- [插件平台规格](../../specs/plugin-api-platform.md)、[领域约定](../../agents/domain.md)和[议题工作流](../../agents/issue-tracker.md)约束通用能力、术语及发布行为。
- [B3 双栏简洁版](run-config-simple-design.md)与[B3 验收记录](../verification/run-config-simple-2026-10-06.md)是此前实现的历史证据，不是本轮插件表单和配置树已实现的证明。
- [目标发现与准备决定](run-debug-build-targets.md)及[调试提供者与握手决定](run-debug-build-debugger.md)是既有可复用能力的依据；模板显示名称不修改真实构建产物和单目标调试的责任边界。
- 当前分支历史文档仍使用旧归档位置；引用按实际存在的文件核对，不借此次规格整理重排其他目录。未发现额外独立 ADR 或 CONTEXT 文档，不虚构决策来源。

### 技术细化边界

能力名称和版本、字段编码、校验等待预算、树持久化格式、插件表单与草稿交换的具体消息，应在现有公开接口附近设计并测试。规格规定外部语义，不指定未经讨论的协议字段、固定超时数字或实现文件布局。技术细化不得降低上述行为；确需改变产品决定时须明确指出差异。

### 发布状态

2026-10-07 用户确认拆分、阻塞关系及主要测试入口，并要求发布后依次执行。规格已发布为 [#67](https://github.com/T-miracle/Editor/issues/67)，[4 张实施工单](../tickets/run-config-plugin-tree/README.md)已发布为 #68–#71；正文、标签与原生阻塞关系已读回确认，实际映射保存在工单目录的 publication.json。实施状态以工单索引和验收记录为准，发布不代表功能完成。
