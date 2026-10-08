# 宿主消息窗口规格

日期：2026-10-07

稳定标识：`ui-host-messages-window`

状态：产品行为与应用级测试接缝已确认（2026-10-07），待实施。本文不表示功能已实现或通过验收。

议题：[设计规格 #84](https://github.com/T-miracle/Editor/issues/84)，标签 `ready-for-agent`，正文与标签已读回核对。用户已要求按少量垂直切片起草实施工单，[两单拆分草案](../tickets/host-messages/README.md)待批准；本次不启动实现、不提交代码。

## Problem Statement

宿主的操作结果、警告和错误目前分散在即时提示和状态信息中。用户需要一个可随时打开、回看和清空的消息窗口，避免错过已经消失的提示，也避免查看消息打断编辑。

插件已有独立的日志管理入口。把插件日志与宿主消息重新混合、按插件再分组，会重复已有职责。本次消息窗口只管理宿主本身的消息。

## Solution

在主窗口内新增宿主自带的原生消息窗口组件，首次使用时默认显示在主窗口右侧。窗口可以收起，通过底部状态栏的消息按钮重新打开，后续恢复用户保存的布局、宽度与显隐状态。

消息按钮使用用户提供的“通知.svg”图标。宿主警告或错误到达时，在按钮图标上显示红色圆点；普通消息不触发红点，不使用计数徽标替代圆点。消息到达不自动展开窗口或抢焦点。打开消息窗口会清除已有红点，之后收到新的警告或错误可以再次触发红点。

窗口使用不分组的消息列表，最新消息在上。初始最多展示 20 条，存在尚未展示的历史时显示“查看更多...”文本按钮，每次追加展开 20 条更早的消息，最后不足 20 条时展示剩余记录。

本次运行最多保留最近 500 条消息，超过上限移除最早记录。收起窗口不清空消息；关闭编辑器后不保留消息历史。提供“清空消息”按钮，一次清除所有宿主消息和红点。

## User Stories

1. As an editor user, I want a host-owned message window, so that I can review editor messages without installing a plugin.
2. As an editor user, I want the message window on the right when first used, so that I can find it beside my editing area.
3. As an editor user, I want host operation results retained, so that I can review completed actions after an immediate notice disappears.
4. As an editor user, I want host warnings retained, so that I can investigate conditions requiring attention.
5. As an editor user, I want host errors retained, so that I can inspect failed operations.
6. As an editor user, I want plugin messages to stay in plugin log management, so that the two message sources have distinct destinations.
7. As an editor user, I want one ungrouped host message list, so that I can browse messages without selecting categories.
8. As an editor user, I want cursor changes and live progress excluded from history, so that frequent state updates do not flood the list.
9. As an editor user, I want internal debug traces excluded, so that the window remains focused on user-facing messages.
10. As an editor user, I want the newest messages at the top, so that recent results are easy to find.
11. As an editor user, I want only the first 20 messages displayed initially, so that the first view remains manageable.
12. As an editor user, I want a “查看更多...” text button, so that I can choose when to reveal older messages.
13. As an editor user, I want each expansion to reveal up to 20 additional messages, so that I can review history incrementally.
14. As an editor user, I want the expansion button hidden when all retained messages are displayed, so that it does not offer an unavailable action.
15. As an editor user, I want at most 500 messages retained, so that history does not grow indefinitely during a long session.
16. As an editor user, I want the oldest messages removed first at capacity, so that the latest 500 remain available.
17. As an editor user, I want hiding the window to preserve its messages, so that I can return to the same session history.
18. As an editor user, I want a fresh message history after restarting the editor, so that old session messages do not persist.
19. As an editor user, I want to collapse the window, so that I can reclaim editing space.
20. As an editor user, I want a bottom status-bar button to reopen it, so that the window remains accessible while hidden.
21. As an editor user, I want the provided notification icon on that button, so that I can recognize the intended entry point.
22. As an editor user, I want layout, width and visibility restored, so that the window follows my last arrangement.
23. As an editor user, I want a red dot for a new warning or error, so that I can notice important host messages.
24. As an editor user, I want ordinary messages to avoid raising that dot, so that routine results do not demand attention.
25. As an editor user, I want new messages to leave my focus and window visibility unchanged, so that I can continue editing.
26. As an editor user, I want opening the window to dismiss the existing dot, so that I do not need to acknowledge messages individually.
27. As an editor user, I want later warnings and errors to raise the dot again, so that opening the window once does not suppress future reminders.
28. As an editor user, I want a clear-all action that also clears the dot, so that I can start observing from an empty history.
29. As an editor user, I want clearing host messages to leave plugin logs untouched, so that independent diagnostic history is preserved.
30. As an editor user, I want controls and messages usable in Chinese and English and in both themes, so that the window follows the editor's existing presentation settings.

## Implementation Decisions

### 所属模块与边界

- 消息窗口属于宿主应用外壳及原生 UI 层，是主窗口内的组件，不是插件，也不是新增操作系统级独立窗口。
- 复用现有 DockArea、宿主面板身份、底部状态栏及会话布局恢复机制，不建立另一套停靠状态。
- 可见按钮、列表、滚动与窗口外观遵循现有 gpui-base 行为基座和本地控件、主题契约；保留深浅主题及中英文资源。
- 宿主消息由应用持有并集中发布，具有明确的普通、警告、错误等级；不通过文案关键词猜测等级，不把渲染刷新当作新消息。
- 插件日志仍由现有插件日志管理负责，包括归属于插件的运行输出、操作结果和错误。宿主发出的插件相关日志不因发出方是宿主就复制到本窗口。
- 不新增插件 API、插件能力或插件包，不将本窗口伪装为特殊插件，也不改变已有插件日志的已读、提醒和容量规则。

### 消息内容与生命周期

- 收录宿主面向用户的操作结果、警告和错误；光标位置、实时进度等高频状态及内部调试日志不进入消息历史。
- 仅在当前编辑器运行期间保留消息，容量统一为 500 条，所有等级共同计数。第 501 条到达时移除最早一条，以此类推。
- 容量上限与单次展示数量独立：保留最多 500 条，不等于一次渲染全部记录，也不沿用插件日志的 512 条上限。
- 消息历史不写入持久化会话；可持久化的是窗口布局、宽度和显隐状态。重新启动后不恢复上一次运行的消息及红点。
- 隐藏、重新打开或调整窗口大小不删除消息。明确清空操作才清除全部宿主记录。

### 列表与展开

- 一个消息列表，不设置“默认”“插件”或其他来源分组。
- 按宿主接收顺序倒序显示，最新消息在上；同一时刻的消息也应保持确定顺序。
- 初始展示最近 20 条，不足 20 条则全部展示。
- “查看更多...”每次追加最多 20 条更早的记录，保留已经展开的内容，不跳到另一页；全部展开后不再显示该按钮。
- 清空后展示空状态，不保留失效的展开入口；后续新消息继续按正常规则接收和展示。
- 新消息到达与超限淘汰不能导致列表重复记录、显示已淘汰消息，或使展开动作越过 500 条保留边界。

### 显隐、提醒与清空

- 首次使用默认在主窗口右侧显示，之后遵循已保存的布局、宽度和显隐选择。旧布局没有该面板时，需要补入新面板而不覆盖用户已有面板布局。
- 收起窗口回收对应空间，底部消息按钮保留；通过按钮重新打开窗口。
- 底部按钮采用用户指定的通知图标，红色圆点叠加于该图标；不加入数量徽标或自动弹窗。
- 普通消息不触发红点；新警告或错误触发红点，不自动展开或抢焦点。
- 打开窗口只确认当时已经到达的提醒。确认之后到达的新警告或错误仍可以显示红点，不能被先前打开操作的延迟回调误清除。
- 红点清除不要求逐条点击、滚动到所有记录或展开全部历史，也不等于删除消息。列表重绘本身不自动清除后续提醒。
- “清空消息”一次清除全部宿主消息与已有红点，不改动插件日志；清空之后到达的消息属于新的历史与提醒。

## Testing Decisions

### 主要测试接缝（已确认）

优先采用一条现有应用级纵向路径：**宿主操作产生消息 → EditorApp 主窗口集成 → 原生消息按钮与面板交互 → 可见列表、红点和布局结果**。

- 复用现有 GPUI TestAppContext / VisualTestContext、隔离临时工作区、应用窗口构造与真实鼠标、键盘、滚动和尺寸变化输入。
- 消息来源集成至少覆盖真实宿主操作的成功与失败路径；容量和批量展开场景可通过同一生产消息发布入口送入可控记录，不逐条依赖外部失败，也不为测试创建第二套消息存储或插件专用入口。
- 主测试只断言用户可观察行为：实际可见文本及顺序、条数、按钮状态、红点、焦点和窗口占位，不把私有集合形状、内部方法调用次数或字段命名作为验收依据。
- 必要的容量或顺序边界测试只能补充这条应用级路径，不能替代真实按钮、消息来源和布局验证。
- 沿用现有插件管理器及日志入口做隔离回归，验证插件日志和宿主消息互不读取、清空或触发对方提醒。

### 可借鉴的现有先例

- 应用窗口测试通过真实按钮输入打开设置窗口，并验证可见布局与保存选择。
- 原生停靠拖动测试通过实际指针拖动验证尺寸变化、收起后的空间回收和恢复尺寸。
- 插件状态栏测试通过原生输入检查提醒确认边界与新到消息；这里只借鉴测试方式，不共享插件日志状态。
- 插件日志管理测试已有长列表滚动、最新记录置顶及迟到消息提醒的先例，可复用场景设计。

### 验收矩阵

| 编号 | 场景 | 可观察结果 |
| --- | --- | --- |
| M01 | 无保存布局时启动 | 消息窗口默认位于主窗口右侧；无需安装插件 |
| M02 | 宿主操作成功、警告与失败 | 相应消息进入宿主列表；高频状态及内部调试输出不进入历史 |
| M03 | 插件产生消息或错误 | 保留在插件日志入口，不进入宿主列表，也不触发宿主消息红点 |
| M04 | 0、1、20、21、40、41 条消息 | 初始最多 20 条；每次展开最多 20 条；余数正确且无更多记录时不显示展开按钮 |
| M05 | 多条消息连续到达 | 最新在上，同时间接收也有稳定顺序，无重复记录 |
| M06 | 达到 500 条并继续接收 | 仅保留最近 500 条，普通消息与警告、错误共同遵守上限 |
| M07 | 已展开历史时继续收消息或发生淘汰 | 展开结果无重复或已淘汰记录，查看更多仍遵守保留边界 |
| M08 | 收起与重新打开 | 消息仍在；编辑空间回收；按钮能够恢复窗口 |
| M09 | 调整宽度、显隐后重启 | 恢复保存布局、宽度与显隐；不恢复历史消息及红点 |
| M10 | 加载没有消息面板的旧布局 | 补入消息面板，不覆盖或丢失已有宿主与插件面板布局 |
| M11 | 普通消息到达 | 不出现新的红点，不改变焦点或自动展开 |
| M12 | 警告或错误到达 | 通知图标显示红色圆点，不显示数量；不抢焦点或自动展开 |
| M13 | 打开有红点的窗口 | 已有红点消失，记录保留；不要求展开全部历史或逐条点击 |
| M14 | 打开确认后收到新警告或错误 | 再次显示红点，旧确认与普通重绘不能抹掉新提醒 |
| M15 | 清空含消息和红点的窗口 | 宿主列表为空且红点消失；插件日志不变；新到消息仍正常接收 |
| M16 | 深浅主题、中英文、键盘与滚动、缩放 | 指定图标、红点、按钮及文本可辨识；操作可达；列表滚动与编辑焦点正常 |
| M17 | 受限工作区 | 宿主消息组件可用，不因查看消息启动插件或语言工具 |

实施时按仓库约定执行 Rust 格式检查、非 UI workspace 测试、workspace 编译检查，以及相关 editor-app 针对性测试和 Windows 原生交互验收。对本次文档交付只检查链接、路径、现行契约一致性与差异空白，不声称上述验收已执行。

## Out of Scope

- 插件消息汇总、插件分组、跨来源统一日志中心，以及替换现有插件日志管理。
- 首轮访谈提到的“默认 / 插件”分组和每组 512 条方案；它们已被用户后续明确修正取代。
- 消息跨重启保存、磁盘日志归档、消息导出、远程上传及跨设备同步。
- 高频编辑状态、实时进度流、内部调试日志和完整控制台输出。
- 自动展开窗口、焦点抢占、声音、系统通知或未读数量徽标。
- 消息搜索、过滤、逐条删除、逐条已读等未经确认的附加管理能力。
- 新增独立原生窗口、自由停靠体系、插件协议扩展、插件版本发布。
- 本次直接实现功能、执行其他工单、提交或推送代码。

## Further Notes

- 用户最后的范围修正优先于早期访谈建议：仅宿主、不分组、500 条；“每次 20 条”和“查看更多...”保留。
- 图标原始附件为“通知.svg”，已随方案保存在 [通知图标参考资源](assets/host-messages-notification.svg)。它是视觉素材，不是运行指令；实现时纳入应用静态资源，并遵守本地图标与主题契约。
- 代码库依据：[原生 UI 目录](../README.md)、[需求基线](../../project/需求整理.md)、[开发计划](../../project/开发计划.md)、[领域词汇约定](../../agents/domain.md)、[议题跟踪器](../../agents/issue-tracker.md)、[分诊标签](../../agents/triage-labels.md)。
- 测试先例：[应用窗口与设置](../../../crates/editor-app/src/tests/mod.rs)、[原生停靠交互](../../../crates/editor-app/src/extensions/dock_tests.rs)、[插件状态栏](../../../crates/editor-app/src/app/plugins/status_tests.rs)、[插件日志管理](../../../crates/editor-app/src/extensions/management_tests.rs)。这些路径仅用于定位现有证据，不约束未来模块布局。
- 用户已确认应用级测试接缝，并要求工单不要拆得过细、减少重复测试。方案议题发布后，实施工单按批准的拆分发布；不把发布等同于功能交付。
