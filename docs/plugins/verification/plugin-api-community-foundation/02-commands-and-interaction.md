# 02：命令、原生交互与选择授权验收

日期：2026-10-09。对应 [工单 02 / GitHub #94](https://github.com/T-miracle/Nanobug/issues/94)、[实施正文](../../tickets/plugin-api-community-foundation/02-commands-and-interaction.md)与[总方案](../../specs/plugin-api-community-foundation.md)。

状态：实现与本单自动验收已完成，候选等待根代理原生验收和 Standards / Spec 双轴审查；未推送、未关闭 issue。下文的 GPUI 测试与实际桌面观察分别记载，不能以编译或旧 exe 的观察代替最终原生验收。

## 固定输入与构建

工作树 `C:/Users/Tmiracle/.codex/worktrees/plugin-api-interaction/Editor`，分支 `codex/plugin-api-interaction`，基线 `452994e6d3d6da526803cc62db96ee722580e670`。最终候选提交由同一分支的提交记录确定；本记录随候选提交，避免把自身未形成的 commit hash 写成证据。

工具链：`rustc 1.98.1 (48a229cea 2026-09-01)`，`cargo 1.98.1 (797e8a9bc 2026-08-05)`；已安装目标 `x86_64-pc-windows-msvc`、`wasm32-wasip2`。没有安装工具链、修改全局环境或调用历史脚本。宿主 SDK `plugin-protocol 0.2.0`、wire protocol `7`；新增能力 `plugin.commands 1`、`ui.interaction 1`、`files.selection 1`。依赖复用锁定的 `gpui-base / gpui-kit 0.7.0`、`raw-window-handle 0.6.2`、`windows 0.62.2`。

在上述工作树直接执行：

```powershell
$env:CARGO_TARGET_DIR='C:/Users/Tmiracle/.codex/worktrees/plugin-api-interaction/target'
cargo build -p editor-app
& '../target/debug/editor-app.exe' --plugin-package plugins/capability-example plugins/example plugins/terminal --output target/plugin-api-test
& '../target/debug/editor-app.exe' --plugin-cargo plugins/terminal/Cargo.toml test --lib
```

三包均由当前宿主内嵌 SDK 缓存独立编译并正式打包；访客无仓库 crate 路径依赖或 SDK 源码副本。SDK 缓存摘要 `22751a70f284b7363e30323353a4c2dff1b29464c3b94d6889bd68852f457062`。终端本次只适配新增可选命令上下文字段，独立单元测试 **44 passed**。

| 产物 | SHA-256 |
| --- | --- |
| `target/plugin-api-test/capability-example-0.18.0.zip` | `1cd999f59dab8fc0790c0a025b1f6ec99a6b9d3e2e871dc95761059b750a5c63` |
| `target/plugin-api-test/example-0.4.0.zip` | `d775e2d12d94963e8b1910ef3e245f03037bbda6cefc95fb012880e852b596cd` |
| `target/plugin-api-test/terminal-0.12.3.zip` | `df8fa6ed9c53732d0497c543a65af1722dcada12b639d9f4b9e8678e57c01607` |
| `../target/interaction-candidate/editor-app.exe` | `e62ec53fba4d73da50658d0b2c175d3bbee5f8bd1b86c3ef1f1d5a9ec4d5b4ae` |

该 exe 是当前构建产物的独立副本，用于原生验收，不锁住 Cargo 输出、不接触用户主工作区实例。后续若修改实际源码/SDK/清单，需重新核对受影响证据与指纹。

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
| T08/T10：本地交互控件 | **2 passed**。实际 SDK 包和 Base Input handler 的 marked text 不被第一次 Enter 确认；提交中文后下一次 Enter 返回真实文字。512 项快选 End 将最后条目滚入可见区，确认按钮仍可见，Enter 返回 `511`。Base 已绑定的移动 Action 与 raw key 共用有 IME 检查的导航；条目具有可读 accessibility label。 |
| T09：`selected_resources` | **12 passed**。文件精确读取、目录相对子项、Save 无读写旁路；cross-instance/伪造/释放/禁用/信任撤销；取消、超时、非法整批选择不留授权；路径穿越、设备/ADS/绝对路径与真实 Windows junction 逃逸被拒绝。选中后正常改名/原子替换成功，旧句柄拒读新对象。普通 service 与双方显式具有 `files.select` 的跨插件 typed command 都不能选择或读取；公开 Manager 直接 typed 调用可选择、读取并释放自身资源。 |

命令与 UI 结果经 `Accepted -> RequestUpdate` 的真实 transport 返回；没有测试专用宿主命令 API 或插件 ID 白名单。重复测试仅用于处理失败及新增的来源/键盘边界，未为后续每个插件复制完整原生矩阵。

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

根代理随后对上表最终 `e62ec53...` exe 完成真实 Windows 复查：panel 菜单在按钮下方右侧；QuickPick 暴露“快速检查 / Quick”“完整检查 / Full”名称，Down 实际切到 Full。另一次默认 Quick 流程中，物理 `n`、`i`、空格提交“你”，Confirm 呈现“你 (brief)”，随后 50% 非模态 Progress 不抢焦点；点击取消回显 `Cancelled / cancelled / not_executed` 且控件撤销。操作超过 30 秒也实际返回 `timed_out`。最新候选的文件/目录/保存系统选择、四类菜单完整入口、相关缩放与退役可见结果仍待根代理完成记录。

工单 01 合入后的虚拟只读文档菜单需按资源身份适配 `path=None`，这是根代理集成回归，不能在本单独立树把虚拟资源伪装为磁盘路径。最新原生 C03、集成复核与双轴审查尚未记为通过，因此此记录不授权提前关闭 #94。
