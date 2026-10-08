# 宿主大纲工单 03 验收记录

日期：2026-10-07–08。对应 [#75](https://github.com/T-miracle/Editor/issues/75)、[工单](../../tickets/xml-language-tools/03-outline-and-docking.md)与[方案](../../specs/xml-language-tools.md)。

状态：已交付；本单定向行为验收、最终共同门禁、SDK 分发、SVG 短组合与两个独立审查轴均通过。实现提交 `2ba72a0c866478353bdbe350b89ae4c79a974fd0` 已推送并核对远端 SHA；#75 于 `2026-10-07T16:16:44Z` 关闭，已读回 `closed/completed`。最终命令、产物与审查身份见[共同交付记录](02-03-delivery.md)。

## 验证边界

- Windows x86_64 MSVC；独立工作树 `codex/xml-language-tools`。原生 Cargo 使用本工作树 `target/native/`，与并行工单串行安排编译、链接和 GPUI 执行。
- XML 结构与格式化/标签编辑共同交付包版本 `0.2.0`；结构依赖独立 `language.structure ^1`，不为大纲声明假的 LSP 或 process 权限。
- `structure::Request/Proposal` 经正式内嵌 SDK 分发；纯 worker 复用既有不可变语言快照执行底座，新增独立 typed 输出通道。结构读取只获得 EditorState 快照和有效设置，不具有文件、资产、UI、进程或文本写入操作。
- 模型校验目标文档 incarnation、revision、request nonce 与提供者 Arc 生命周期；大纲和折叠使用覆盖、定义定位和完整折叠三种独立范围。图标仅从该提供者的确切已安装包读取并验证，缺失/不安全 SVG 字节使用本地默认定义图标。
- 原生大纲借用 Base TreeState 的展开、选择、键盘和滚动，DockArea 是唯一布局/尺寸保存真相。顶部通过其标准 center 树的 Top 分割表达，没有第二份“顶部布局”。
- 公共 Manager 是实际 XML ZIP 和陌生语言 ZIP 的入口；跨功能 GPUI 用例复用既有测试发布夹具，未扩大生产 Worker 可见性或建立 XML 专属测试 API。

## 已执行记录

| 验证 | 实际入口与结果 |
| --- | --- |
| 实际 XML 旧组件 RED | 公共 Manager 调用新增结构回调，旧实际 WASM 拒绝 `LanguageStructure` 变体；保留结构 capability 接入前的失败证据 |
| 当前 SDK 分发 RED/GREEN | 独立 XML 构建先因内嵌 SDK 缺 `src/structure.rs` 失败；补 SDK_FILES 后协议依赖独立编译通过，再修正 XML `BytesStart::as_ref()` 类型歧义与 0.41 属性规范化接口 |
| XML 正式 ZIP 构建 | `./scripts/build-plugins.ps1 -HostExe target/native/debug/editor-app.exe -Packages xml`：GREEN，4.19 s，实际 `dist/plugins/xml.zip` |
| 陌生 guest 正式 SDK 构建 | `./scripts/build-capability-example.ps1 -HostExe target/native/debug/editor-app.exe`：GREEN，15.26 s，夹具版本 `0.17.0`；仅已有 service_demo 的 dead_code 警告 |
| 超深回复增量夹具构建 | 同一公开 SDK 入口：GREEN，7.31 s；陌生 guest 的 `overdepth` 返回 140 层 typed 节点，供有限 JSON 深度与异常后不静默重启回归，不新增宿主插件特判 |
| XML 独立结构、范围、注释折叠和图标 | `cargo test -p plugin-runtime --test xml_structure -- --ignored --nocapture`：1 passed，0 failed，47.84 s。重新打包实际 XML 组件仅移除 LSP/process 声明，通过公共安装器观察元素顺序、中文/emoji id/name、精确开始标签名、覆盖范围、独立注释折叠、包图标和停用后旧 Arc 拒绝 |
| 陌生结构、恶意回复与纯 IO 拒绝 | `cargo test -p plugin-runtime --test structure -- --ignored --nocapture`：2 passed，0 failed，214.37 s。实际独立 guest 的任意类型名称、缺失/不安全 SVG 兜底，旧 revision、非法 UTF-8 边界、范围越界、遍历路径、4,097 节点拒绝；即使正常 guest 有 IO 授权，纯回调读取 workspace/data/assets 仍失败 |
| 生命周期和不变发布 | 同一陌生 guest 真实用例：相同发布保留 Arc，设置替换、信任撤销、恢复信任、工作区切换和卸载撤销旧 Arc；撤权后资源数为 0，后续新实例可正常描述 |
| 实际原生 trap 清理 | `tests::outline::lifetime::native_structure_trap_clears_tree_without_another_document_event`：在四项原生首轮中实际 `ok`。独立 SDK guest 正常描述后，原生输入触发真实 WASM panic；没有下一次文档操作，旧树撤销，纯句柄 inactive，原生空状态可见，loading 不残留，文本保持 `trap`。该轮其他两个 XML 用例失败后，精确终止自有测试进程，不能把部分结果计为整轮通过 |
| 侧栏方向原生 RED | `cargo test -p editor-app native_horizontal_splits_reject_tab_merges --no-default-features -- --nocapture`：1 failed；原生指针拖到右侧区域后预览 `120×672`，而上下排列要求 `240×336`。本地 renderer 后续统一 drop preview 与公开 split 编辑的方向策略 |
| 本地 DockArea GREEN | `cargo test -p editor-app 'ui::controls::dock::tests' --no-default-features -- --nocapture --test-threads=1`：5 passed，0 failed，0.16 s（编译 31.13 s）。真实四边鼠标预览/释放、没有下一帧的 immediate-release、中部 tab 合并拒绝、独立面板叶及嵌套 split 保留 |
| 实际应用四向布局与恢复 GREEN | 三项定向应用轮中 `native_outline_four_directions_resize_and_workspace_restore` 完整 `ok`：同一实际 XML Manager 逐个覆盖左/右/顶/底 title drag、原生预览/释放、Explorer 加入后对应排列与均分、真实 45px divider resize、未保存源码刷新保留比例、重新打开后的实际位置/尺寸恢复；该轮整体 1 passed/2 failed、170.65 s，不宣称整轮通过 |
| 连续输入期间的展开状态 RED/GREEN | 单项原生导航 RED：0 passed/1 failed、41.74 s，连续两个原生字符 revision 的 loading 均已显示，最终 label 正确但手动展开丢失。修复等待中空树覆盖保存 flags 后，同一导航在两项原生轮完整 `ok`；整轮 1 passed/1 failed、88.36 s（另一项仍为首次 Rust gutter），未重复四向布局 |
| 结构 JSON 专用有界解码 | `cargo test -p plugin-runtime --lib 'instance::structure::tests' -- --nocapture --test-threads=1`：2 passed、0 failed、0.00 s（编译 28.05 s）。显式 2 MiB 栈解码/释放 128 层 typed 节点；相同数据被默认 generic decoder 拒绝，超深、字节超限、尾随和非法 escape 均拒绝 |
| 真实首帧初始化折叠 RED/GREEN | `cargo test -p editor-app folds_ -- --ignored --nocapture --test-threads=1`：帧驱动前普通 Rust 的 `language_name()` 仍为 `text`，夹具补公开 `simulate_next_frame` 后普通 Rust 基线通过，但 XML 原生注释 gutter 失效；确认零范围高亮 refresh 被误当文本改动。最小条件修复后同轮 2 passed、0 failed、49.56 s（编译 32.57 s），日志 `03-outline-fold-source-green.log` |
| 深语法树遍历和折叠所有权 | 上述两项 GREEN：动态 Rust WASM 的千层 block 真实树在显式 2 MiB 栈中经 TreeCursor 收集/排序/去重和 Drop；应用原生注释 gutter、中文多行输入后新 revision、异步语法完成、切换普通 Rust、停用 XML 的普通 Rust gutter 均有效。深 XML grammar 的首个夹具实际生成浅层 ERROR 恢复树，改为真实深 Rust 树验证通用遍历，不把浅树当作深度验收 |
| XML 实际包深层结果和源限额 | `cargo test -p plugin-runtime --test xml_structure xml_structure_deep_documents_stay_bounded_on_two_mib_stack -- --ignored --nocapture --test-threads=1`：1 passed、0 failed、43.51 s（编译 14.44 s）。公共 Manager 安装/结构执行/校验/Drop 全部位于显式 2 MiB 线程；60/127 层中文元素的完整 definition 与 fold 数正确，128 层源码有限 127 节点且无猜测折叠；1 MiB+1 输入在 dispatch 前拒绝，同一健康 Arc 保持 |
| 实际异常退役、超深拒绝和恢复 | `cargo test -p plugin-runtime --test structure independent_structure_trap_retires_its_lease_and_configuration_recovers -- --ignored --nocapture --test-threads=1`：1 passed、0 failed、90.03 s（编译 3.00 s），日志 `03-structure-trap-depth-green.log`。实际 WASM trap 撤销 retained Arc 和一份纯 worker 资源，不静默重启；显式 project 配置替换得到健康新 Arc，再用 140 层 encoded JSON 触发有界 decoder 明确 nesting 错误、再次退役。两个故障均保留正常 owner 和其他角色 |
| 首次完整 App 门禁失败及后续 GREEN | 主代理 `final-app-suite-before-outline-migration-fixture.log`：370 passed、1 failed、160 ignored、133.70 s；唯一失败为 `app::session::tests::plugin_dock_layout_survives_delayed_startup` 的旧完整树预期未含大纲迁移。实际保存的 Explorer 区域宽度、Editor/tasks 分割尺寸及终端右侧 317px/关闭状态都保留，差异只有区域内新增等分 Outline。保留旧夹具与启动等待时原始数据断言，仅按保存的相邻列总高度推导新等分树，继续精确比较所有原有字段/尺寸。后续 `final-app-suite.log`：374 passed、0 failed、161 ignored，127.45 s，该完整树比较已通过 |
| 双语 SDK 门禁 | `npm test`（`website/`）：15 passed，0 failed，2 个搜索测试因没有站点构建索引 skipped；本单未修改搜索或版式 |

日志位于 `target/xml-language-tools/03-*.log`。纯 WASM 多次实例准备的实际耗时已如实记录，没有把被跳过的包测试计为通过。深语法树纯检查只依赖仓库现有动态 Rust WASM grammar，与既有普通 parser 测试相同；该例现归普通 App 测试，当前复现命令为 `cargo test -p editor-app grammar_folds_walk_deep_source_without_recursive_stack_growth -- --nocapture --test-threads=1`，实际包/窗口例仍须显式 `--ignored`。

DockArea 实施还通过实际 RED 修正两项接入问题：上游每区域的 last-panel 约束会固定住可跨区域移动的唯一面板，项目继续使用既有锁定/缩放检查；同一元素叠加第二个 `on_drop<DragPanel>` 会二次消费 GPUI drag，改为仅拖动期间存在的独立本地 hit child，一次消费后调用公开 `move_panel`。没有读取 GPUI 私有拖动字段或维护第二份布局树。

完整应用夹具首次 RED 还确认：测试 Manager 的实际包目录必须与既有 `cfg(test)` ExtensionPanel 的 `.runtime-plugin-test` 根一致；此测试目录约定不改变生产私有存储位置。统一根后，XML 的 highlight/recognition/structure 角色均有效，实际大纲 root 已绘制且不在 loading/failed，但 children 不绘制。单项诊断保留 `175×34` 的测量行结果，随后补 Base Tree 公开 `list_style` 的有界窗口尺寸。首轮的 `0xffffffff` 是为避免同一夹具缺陷重复超时而终止自有测试进程，不是产品崩溃证据。

定向导航现已完整通过列表子项、Dock 焦点/方向键、点击精确定位、原生注释 gutter、跟随与独立 disclosure、Enter、中文输入/Undo、连续 pending revision 的手动展开、暗色/DPI2、90 节点虚拟滚动、关闭/重开 incarnation 和停用清理。其间修正三个测试接入错误：陌生包只替换身份而保留 `.xml` 扩展；fold helper 用公开 `range_to_bounds` 的源行实际坐标并先 hover 绘制原生 chevron；长文本通过实际 Clipboard/CtrlV 粘贴成为单次 Undo，而不是把逐字符/换行模拟误认为一个撤销组。旧日志分别为 `03-outline-navigation.log`、`03-outline-navigation-recognition.log`、`03-outline-navigation-geometry.log`，保留失败入口；本轮完整导航结果为 `03-outline-navigation-fold-green.log`。

连续输入还实质修复了产品缺陷：同一文档/提供者的第二次 revision 可能发生在上次结构请求 pending 且树已清空时；此时保留上次已显示树的展开 flags，只有 snapshot 存在时才从当前 Base 树重新采样。文件/提供者切换仍清除 flags，未引入第二份可变文本或私有事件接口。

公开下一帧入口还暴露并修复了结构折叠的真实初始化竞态：Base 为高亮首解析和纯刷新传入零范围 `InputEdit`，其 revision 没有改变。适配器仅在旧范围或新范围非空时撤销候选，避免稍晚的语法初始化抹去已接受的结构结果；真实文本替换仍及时失效并等待该 revision 的新结果。结构退役后的 grammar fallback 以 TreeCursor 迭代遍历，保留 Base 的 named-node 选择、排序和去重规则，不依赖 128 层结构回复限制约束任意语法树。

## 待完成的共同门禁

- 由主代理协调最后一次短 SVG/XML/Image 组合、必要窗口启动、格式、非 UI workspace、workspace check、全部非 ignored App 测试及双轴独立审查；统一结果见[共同交付记录](02-03-delivery.md)，本单不重复抄录全量门禁。结果到齐后再更新工单状态及提交/推送/关闭记录。
