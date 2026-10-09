# 02：命令、原生交互与选择授权验收

日期：2026-10-09。对应 [工单 02 / GitHub #94](https://github.com/T-miracle/Nanobug/issues/94)、[实施正文](../../tickets/plugin-api-community-foundation/02-commands-and-interaction.md)与[总方案](../../specs/plugin-api-community-foundation.md)。

状态：实现与本单自动验收已完成，根代理已记录原生候选复核，最终交付等待集成差异复核和 Standards / Spec 双轴审查；未推送、未关闭 issue。下文的 GPUI 测试与实际桌面观察分别记载，不能以编译或旧 exe 的观察冒充新指纹的物理验收。

## 原始构建与行为验收输入

工作树 `C:/Users/Tmiracle/.codex/worktrees/plugin-api-interaction/Editor`，分支 `codex/plugin-api-interaction`，基线 `452994e6d3d6da526803cc62db96ee722580e670`。最终候选提交由同一分支的提交记录确定；本记录随候选提交，避免把自身未形成的 commit hash 写成证据。

工具链：`rustc 1.98.1 (48a229cea 2026-09-01)`，`cargo 1.98.1 (797e8a9bc 2026-08-05)`；已安装目标 `x86_64-pc-windows-msvc`、`wasm32-wasip2`。没有安装工具链、修改全局环境或调用历史脚本。宿主 SDK `plugin-protocol 0.2.0`、wire protocol `7`；新增能力 `plugin.commands 1`、`ui.interaction 1`、`files.selection 1`。依赖复用锁定的 `gpui-base / gpui-kit 0.7.0`、`raw-window-handle 0.6.2`、`windows 0.62.2`。

在上述工作树直接执行：

```powershell
$env:CARGO_TARGET_DIR='C:/Users/Tmiracle/.codex/worktrees/plugin-api-interaction/target'
cargo build -p editor-app
& '../target/debug/editor-app.exe' --plugin-package plugins/capability-example plugins/example plugins/terminal --output target/plugin-api-test
& '../target/debug/editor-app.exe' --plugin-cargo plugins/terminal/Cargo.toml test --lib
```

三包均由原生验收候选宿主的内嵌 SDK 缓存独立编译并正式打包；访客无仓库 crate 路径依赖或 SDK 源码副本。三包构建使用的 SDK 缓存摘要 `22751a70f284b7363e30323353a4c2dff1b29464c3b94d6889bd68852f457062`。终端本次只适配新增可选命令上下文字段，独立单元测试 **44 passed**。

下表保存原始行为测试的输入指纹。C04 审查修正后的当前 SDK、宿主和三包另列于末节；同名包输出已被最新重建覆盖，不能把原始 hash 当作当前文件 hash。

| 产物 | SHA-256 |
| --- | --- |
| `target/plugin-api-test/capability-example-0.18.0.zip` | `1cd999f59dab8fc0790c0a025b1f6ec99a6b9d3e2e871dc95761059b750a5c63` |
| `target/plugin-api-test/example-0.4.0.zip` | `d775e2d12d94963e8b1910ef3e245f03037bbda6cefc95fb012880e852b596cd` |
| `target/plugin-api-test/terminal-0.12.3.zip` | `df8fa6ed9c53732d0497c543a65af1722dcada12b639d9f4b9e8678e57c01607` |
| `../target/interaction-candidate/editor-app.exe` | `e62ec53fba4d73da50658d0b2c175d3bbee5f8bd1b86c3ef1f1d5a9ec4d5b4ae` |
| `../target/interaction-review-candidate/editor-app.exe` | `27f1ab94e2140fa4a86d05a53c3923dc26480056aa043320a707e1b32d1c76dc` |

两份 exe 都是对应构建产物的独立副本，不锁住 Cargo 输出、不接触用户主工作区实例。`interaction-candidate` 对应下文已经实际观察的原生行为；`interaction-review-candidate` 是两条 P3 审查修正后的新构建，差异与复用范围见末节。后续若修改实际源码/SDK/清单，需重新核对受影响证据与指纹。

## 主责自动证据

```powershell
cargo test -p plugin-runtime --test host_interaction --test selected_resources -- --ignored
cargo test -p editor-app extensions::interaction_tests -- --ignored
cargo test -p editor-app ui::controls::interaction::tests -- --ignored
```

| 范围 | 实际结果与可观察断言 |
| --- | --- |
| T06：`host_interaction` | **4 passed**。真实包进入公开 Manager；发现返回精确参数/结果 schema，内部命令通道不出现在服务提供者设置；两实例验证参数、错误、返回形状、权限交集、禁止借用私有根、取消、迟到回复、超时及禁用。取消 typed wait 在下一次 Manager tick 前已关闭所属原生 request，未把 wait 取消误作进程生命周期撤销。 |
| T07/T08/T10：`extensions::interaction_tests` | **2 passed**。真实 Explorer 按钮点击经过正式 worker 路由，保留已打开但非活动文档目标；四类菜单声明的条件/分组排序、真实选区、禁用状态及过期实例移除正确。另两种包身份经生产 publication 接缝验证中文输入返回、稳定快选 ID、确认取消、通知关闭、进度更新/取消、深浅主题与编辑器焦点恢复。 |
| T08/T10：本地交互控件 | **2 passed**。实际 SDK 包和 Base Input handler 的 marked text 不被第一次 Enter 确认；提交中文后下一次 Enter 返回真实文字。512 项快选 End 将最后条目滚入可见区，确认按钮仍可见，Enter 返回 `511`。该快选用例随后合并 en/zh-CN、深浅主题与 1/1.5/2 倍 DPI 的四组实时状态，精确检查译文，英文 Confirm 与中文确认的实际按钮宽度变化，最后选项位于 choices 可见区、取消与确认处于窗口范围；扩展后定向重跑 **1 passed / 0 ignored**。Base 已绑定的移动 Action 与 raw key 共用有 IME 检查的导航；条目具有可读 accessibility label。 |
| T09：`selected_resources` | **12 passed**。文件精确读取、目录相对子项、Save 无读写旁路；cross-instance/伪造/释放/禁用/信任撤销；取消、超时、非法整批选择不留授权；路径穿越、设备/ADS/绝对路径与真实 Windows junction 逃逸被拒绝。选中后正常改名/原子替换成功，旧句柄拒读新对象。普通 service 与双方显式具有 `files.select` 的跨插件 typed command 都不能选择或读取；公开 Manager 直接 typed 调用可选择、读取并释放自身资源。 |

命令与 UI 结果经 `Accepted -> RequestUpdate` 的真实 transport 返回；没有测试专用宿主命令 API 或插件 ID 白名单。重复测试仅用于处理失败及新增的来源/键盘边界，未为后续每个插件复制完整原生矩阵。

双语/缩放扩展的定向命令为 `cargo test -p editor-app native_plugin_quick_pick_scrolls_the_keyboard_target_into_view -- --ignored`，日志 `../target/app-interaction-locale-scale-final.log`。首轮尝试读取 `Window::debug_a11y_tree_json`，但 GPUI TestPlatform 没有激活辅助技术客户端，返回 `None`；没有把此失败当作产品可访问性通过。最终自动断言使用精确翻译与实际布局，真实可访问名称由下文的 Windows 桌面观察验证；此次追加仅改变测试和验收记录，宿主、SDK 与三包指纹保持上表值。

### Windows 系统对话框

根代理在同一工作树实际执行以下平台回归，交接后平台行为源码没有再修改：

```powershell
$env:CARGO_TARGET_DIR='C:/Users/Tmiracle/.codex/worktrees/plugin-api-interaction/target-picker'
cargo test -p platform-windows file_picker::tests -- --nocapture --test-threads=1
cargo test -p platform-windows file_picker::tests -- --nocapture --test-threads=1 --ignored
```

普通平台组 **2 passed / 2 ignored**；显式 ignored 组 **2 passed**，四个真实 modal 窗口覆盖 OpenFile、Directory、Save 和 owner Drop，结果为明确取消，控制 HWND 已销毁。实现采用自己所属 STA 的 `IFileDialog`，取消投递到同线程的控制窗口；GPUI 不阻塞等待，request 终态和 owner Drop 主动关闭 dialog。

初次实现存在启动竞态：`Close` 返回成功却未使 `Show` 返回。不能把这一观察或丢弃 future 当成关闭成功。已用 `IOleWindow` 可见状态、防重入和所属 dialog 消息循环排队 dismiss 修复，再跑上述真实 modal 回归。当前外部 picker 仅实现 Windows；其他平台明确 `UnsupportedOperation`，没有跨平台手测证据。

## 权限与后续接缝

- 命令来源与参数分离；菜单上下文只含描述性的 workspace-relative path、选择/语言/读写状态，不授予文件访问。Application 实例不接收工作区菜单上下文，嵌套/跨插件调用不复制 native menu metadata。
- `files.select` 必须安装授权并协商 `files.selection`。运行时内部 `InvocationOrigin::HostCommand`、精确目标 incarnation、单跳 ancestry 与 effective permissions 共同允许宿主直接命令使用自身选择能力；不信任 guest 字符串 `@host`。普通服务不能声明该权限为可委托 grant。
- 所选资源的路径、种类与实例由宿主不可变绑定。File 使用 `ReadFile(path="")`；Directory 只读合法相对子路径的普通文件；`CloseResource` 释放授权。grant 只保留规范路径与对象身份，闲置时不持有妨碍正常改名/原子保存的锁；读操作短暂 pin 并重验。
- Save 返回精确目标意图，`ReadFile` 拒绝，`WriteFile` 为 `UnsupportedOperation`；03 必须经公开安全事务接缝消费，不绕过打开的脏文档。平台最多四个选择线程和 64 个 native paths，runtime 每批授权最多 32 个，整批校验后才分配。
- 原生请求保持来源、截止时间与第一终态；cancel/timeout/disable/owner close 后迟到 callback 不分配授权。取消待决 UI 不承诺撤销已经完成的副作用，也不终止独立拥有的进程。

SDK 模块、独立构建导出表与[英文/中文站点入口](../../../../website/src/content/docs/en/sdk/index.md)一并更新；SDK 具体用法见 commands / interaction 对应页面。旧单向命令继续保留原语义。

## 仓库交付门禁

以下三项均在本工作树、本单独立 `CARGO_TARGET_DIR` 实际执行成功：

```powershell
cargo fmt --check
cargo test --workspace --exclude editor-app
cargo check --workspace
```

非 UI workspace：**227 passed / 199 ignored / 0 failed**。该命令不会运行 ignored 或 UI 测试；本单上面的 16 项实际包 runtime 测试、4 项应用 ignored 与 Windows 平台 ignored 均另行执行，不计为这 227 项。已有 unused/dead-code/链接警告仍存在，未为消除告警扩大范围。

`website` 内 `npm test`：**15 passed / 2 skipped / 0 failed**；两项搜索索引测试明确依赖尚未构建的站点 dist。本次仅修改 reader Markdown，并已通过结构、双语与链接约定检查，不把未构建的搜索索引称为通过。`git diff --check` 通过。

开发中曾运行过宽的 `native_plugin_` 过滤，额外碰到既有 settings ignored 测试缺少旧名 `capability-example.zip`。本单真实夹具为版本化 `.18.0.zip`，该历史夹具测试未计入本单通过项。菜单初轮暴露 Windows display/canonical path 混用，已修复并用实际非活动会话菜单回归；快选初轮暴露 Base Action 抢先消费导航，已修复并验证第 512 项实际可见。编写测试时的临时编译错误均已修正，以上记录是最终执行结果。

## 桌面验收与未关闭项

根代理已有旧 exe 的真实 Windows IME 探索：拼音组合中的 Enter 只提交文字而保留 Input；空格提交“你”后下一次 Enter 进入显示真实文字的确认，再进入 50% 非模态进度，Cancel 关闭且 guest 收到 cancelled；30 秒 timeout 也移除 UI。这属于旧 hash `0678719fe9bea353893eb89944cd003f6d8320ca3bc6bc519c5b68cd7fde1480` 的差异分析，不代替上表最新候选的完整桌面验收。

同次探索发现 panel 命令菜单错误固定在窗口左上角：旧 `ExtensionPanel.bounds` 没有更新。已改为实际 trigger prepaint 窗口坐标与本地 `PopupMenu::anchor_to`，相关 popup 组 **4 passed**，编译检查通过。

根代理随后对上表最终 `e62ec53...` exe 完成真实 Windows 复查：panel 菜单在按钮下方右侧；QuickPick 暴露“快速检查 / Quick”“完整检查 / Full”名称，Down 实际切到 Full。另一次默认 Quick 流程中，物理 `n`、`i`、空格提交“你”，Confirm 呈现“你 (brief)”，随后 50% 非模态 Progress 不抢焦点；点击取消回显 `Cancelled / cancelled / not_executed` 且控件撤销。操作超过 30 秒也实际返回 `timed_out`。

同一最终 exe 的文件选择也经根代理实际操作：从编辑器菜单打开真正 Win32 picker，输入自有 `input.txt` 绝对路径并点击 Open；返回 panel 与 Notify 均显示 `input.txt:6490 bytes`，与磁盘 6490 字节一致，关闭通知正常。另一轮选择超过 60 秒后，系统 picker 实际自动关闭并回显 `timed_out / not_executed`。根代理集成提交 `586edb2` 的 `native-02.md` 已补齐目录、Save 意图不创建文件、工作区外 `外部😀.txt` 28 字节读取，以及系统取消关闭并返回 `Cancelled / cancelled / not_executed`；其观察指纹仍为 `e62ec53...`。四类菜单、缩放与退役边界结合上文对应的 GPUI / Manager 用例判定。

工单 01 合入后的虚拟只读文档菜单需按资源身份适配 `path=None`，这是根代理后续集成回归，不能在本单独立树把虚拟资源伪装为磁盘路径。根代理已记录本单 C03 原生候选复核；审查修正后的差异复核与双轴审查尚待完成，此记录不授权提前关闭 #94。

## 候选审查 P3 修正

固定候选 `7fa4201` 的规范轴审查指出两项 P3。已删除只转发五个参数的 `reveal_menu_panel`，菜单直接复用 `reveal_plugin_command_panel`；既有方法仅开放为 `pub(super)`，其他接口可见性没有扩大。`EditorOperation::Interaction` 注释现在区分 `Select` 的 `files.selection` 能力与 `files.select` 安装授权，以及其他变体的 `ui.interaction` 能力与授权。英文页面原有说明准确；中文“另需”改为“则需”，与英文的独立授权规则一致。

修正后重新执行三项必需门禁，全部成功：`cargo fmt --check`、非 UI workspace **227 passed / 199 ignored / 0 failed**、`cargo check --workspace`。真实包菜单回归 `cargo test -p editor-app native_plugin_menus_revalidate_context_and_remove_retired_contributions -- --ignored` 为 **1 passed / 0 ignored**，日志 `../target/interaction-review-menu.log`。`website` 的 `npm test` 为 **15 passed / 2 skipped**，仍仅跳过缺少 dist 的搜索索引；`git diff --check` 通过。

随后 `cargo build -p editor-app` 成功，复制到上表 `interaction-review-candidate` 新路径，保留根代理正在验收的 `e62ec53...` 副本。此次只删除同参数转发并校正 SDK 注释与措辞，线协议、schema、权限校验、选择/取消/焦点实现与三包内容均未改变，因此原三包与已有行为测试适用于这一差异；没有重复构建插件包。新 SDK 源文本因注释变化而不同，表中 SDK 缓存摘要仅证明上述三包原始构建。实际 Windows 观察仍属于 `e62ec53...`，没有将其伪称为新 exe hash 的物理验收。

## C04 政策、签名与 SDK 导出补充

Spec 审查追加确认资源释放示例签名与公共兼容政策缺口。已核对 `api/guest.rs` 的 `close_resource(handle: ResourceHandle)`，两语示例均按值传入。双语 SDK 入口现在提供稳定/实验、同 major 加法兼容、required 拒绝/未知 optional 降级、破坏性变化升 major、弃用及迁移的公共政策；最低宿主由当前 `protocol = 7`、`api.base = ^1` 和相关能力协商确定，不猜产品版本，也不新增历史或未来传输协议支持。commands / interaction 两语明确本单三项核心能力为稳定 1.0.0，并链接公共政策。选择器的 Windows 支持与其他平台 `UnsupportedOperation` 已写进公开页面。

公共政策 h2 分别为 `Stability and capability compatibility`、`稳定性与能力兼容`，精确锚点 `/en/sdk/#stability-and-capability-compatibility`、`/zh-cn/sdk/#稳定性与能力兼容`。已将同一政策全文交给 01，要求保留其 documents 链接并同步这一段，避免独立候选引用不存在的锚点或产生第二套政策。

链接检查出现实际红灯：SDK 导出为 **5 passed / 1 failed**，`COMMANDS.md` 指向不存在的 `README.mdinteraction/`；已在 root fallback 之前补齐 commands / interaction 两项页面映射，`cargo test -p editor-app sdk_export::tests` 最终 **6 passed / 0 ignored**，日志 `../target/interaction-review-sdk-tests-final.log`。网站的独立锚点检查通过，但页面检查把 fragment 误作路由；仅修正该测试剥离 fragment，保留单独 heading 校验，最终 `npm test` 为 **15 passed / 2 skipped**，缺少 dist 的搜索索引仍未执行。

SDK 导出修正后重新执行三项必需门禁，fmt 与 check 成功，非 UI workspace **227 passed / 199 ignored / 0 failed**。宿主构建成功，再按正式入口重建三个现有版本包：

```powershell
cargo build -p editor-app
& '../target/debug/editor-app.exe' --plugin-package plugins/capability-example plugins/example plugins/terminal --output target/plugin-api-test
```

实际构建消息中的三包 `plugin-protocol` 路径均指向 SDK key `e6c489a1f6ba891fac413c6658e4cccae50752cdec65a91bf15c71e1bf22cac8`。已直接核对该缓存的 `INTERACTION.md` 包含按值释放、平台失败规则和本地 `README.md#stability-and-capability-compatibility` 链接。曾尝试 `--plugin-cargo ... metadata` 获取 key，该入口只允许 build/check/test 并明确拒绝；该失败没有计入通过，最终 key 取自真正的包构建路径，未扩大工具入口。

| 上一轮 C04 重建产物 | SHA-256 |
| --- | --- |
| `target/plugin-api-test/capability-example-0.18.0.zip` | `a27558f475cf707c26af041d2232493d53d8075d5e0e9c7eae9f2898c0f1f9ef` |
| `target/plugin-api-test/example-0.4.0.zip` | `970e435495328e79854c6673d9e8410950fe0172ea559670337a1e3775db0e54` |
| `target/plugin-api-test/terminal-0.12.3.zip` | `6ede935bd3b0980498375a4642418a1cc101eed7499fb90da96a50c25cf37641` |
| `../target/interaction-docs-candidate/editor-app.exe` | `80621afadc1dba68c284f8727a4e42332c9a6af5f341bd1013998ca6f165de0a` |

此次新增生产代码仅修正 SDK 文档链接导出，访客源码、线协议、schema、权限与 UI 消费行为未变。按根代理明确的复用范围，不重跑完整 negative 或原生矩阵；复用先前相同代码路径的行为断言，当前交付使用新 SDK 与新包指纹。实际 Windows 输入仍绑定 `e62ec53...` 与根代理的原生开发候选，不把它们冒充 `80621a...` 的物理输入。

## 命令提供者 required 声明复核

Spec 复核确认两語命令页对 required / optional 的说明过于宽泛。已静态核对 `package.rs`：有 `signature` 或非空 `menus` 的包均检查 `api.required.contains_key("plugin.commands")`。两语页面现明确提供 typed 命令或原生菜单必须 required；optional 仅供消费者发现/调用的协商降级。公开代码、协议、签名与执行行为没有变化。

此次按根代理限定范围仅执行 `npm test` 与声明静态核对：**15 passed / 2 skipped / 0 failed**，日志 `../target/interaction-provider-policy-website.log`，跳过的仍是缺少 dist 的搜索索引。没有重复全量 Rust、negative 或原生矩阵。英文页面编入 SDK，已重新 `cargo build -p editor-app` 并通过上述正式 `--plugin-package` 入口重建三包；日志分别为 `../target/interaction-provider-policy-build.log`、`../target/interaction-provider-policy-packages.log`。

三包实际构建路径均指向当前 SDK key `312c70d60ab8554f69780169c2a10b81ca81375393f12f842a4b7d0103265900`，已直接核对缓存中的 `COMMANDS.md` 包含提供者 required / 消费者 optional 规则。下表替代上一轮产物的当前指纹，包版本保持本次尚未发布的候选版本；行为与原生输入按未变化的路径复用，不把旧包 hash 当作新包。

| 当前提供者声明复核产物 | SHA-256 |
| --- | --- |
| `target/plugin-api-test/capability-example-0.18.0.zip` | `6d90776018346696197d4ee7c4762a3341b5522a8cfe74e299ecc949fbd59a17` |
| `target/plugin-api-test/example-0.4.0.zip` | `2b2ce9cadd9fcfddaf8a1dd2b2a40423aaad9e97f0087f3138335c2dea483877` |
| `target/plugin-api-test/terminal-0.12.3.zip` | `9f21fdc0dde1d4ede4a401f6ec27e2548b7002ed4536b3ed7f4e6e27267a9639` |
| `../target/interaction-provider-policy-candidate/editor-app.exe` | `349ae9a63d9f0a39bc8accdd0fca795e71b4307fa890e3b2654abd55244616a6` |
