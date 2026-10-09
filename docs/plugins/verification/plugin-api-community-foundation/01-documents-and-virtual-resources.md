# 01：文档快照、事件与只读资源比较候选验收

日期：2026-10-09。对应 [#93](https://github.com/T-miracle/Nanobug/issues/93) 与[工单 01](../../tickets/plugin-api-community-foundation/01-documents-and-virtual-resources.md)。状态：初轮双轴可操作问题已修复，候选门禁与针对性验证通过；受影响 Windows 原生路径已复核通过，双轴复审及推送读回尚未完成，不关闭工单。

## 固定范围与环境

- 批准基线：`452994e6d3d6da526803cc62db96ee722580e670`；候选前执行索引/原生记录提交为 `c65ec2da6e697c612e754ca39cc661ce4c030063` 与 `586edb2`。这些文档没有标记本单完成。
- 工作树：`C:/Users/Tmiracle/.codex/worktrees/plugin-api-community/Editor`，分支 `codex/plugin-api-community`。原始工作区的其他任务改动没有纳入。
- 宿主及仓库 Cargo 检查统一使用 `CARGO_TARGET_DIR=C:/Users/Tmiracle/.codex/worktrees/plugin-api-community/target01`。访客使用宿主正式 `--plugin-cargo` 缓存，不复制 SDK 源码、不依赖仓库 crate 路径。
- Rust `1.98.1 (48a229cea 2026-09-01)`，Cargo `1.98.1 (797e8a9bc 2026-08-05)`；宿主 `x86_64-pc-windows-msvc`，访客 `wasm32-wasip2`，debug 构建。没有安装或升级工具链。
- 修复宿主 `target01/debug/editor-app.exe` SHA256：`95a3332aa41ef23eb8ad0437027144a73a0b852855b67bd27b2de6eeed30de83`；`target/community-review-fix-build.log` 记录构建成功，48.71 秒。原候选 `fc63…` 是初轮双轴与原生复核的对象，不用于替代本次焦点/退役复核。
- 当前 SDK 内容摘要：`df7c52db8ecd3696a8cb4891af75d85d7e1b7a58e91fc8da7b7616375cb5e066`，SDK crate `0.2.0`、基础协议 7；初轮摘要为 `b9b4caa…`。文档能力 `1.1`，只读虚拟资源与比较能力各 `1.0`，均声明稳定核心及最低协商范围。

## 契约和接缝

正式类型位于 `crates/plugin-protocol/src/api/documents.rs`，SDK 导出同时包含该模块与英文 `DOCUMENTS.md`。读者正本为 [English](../../../../website/src/content/docs/en/sdk/documents.md) / [简体中文](../../../../website/src/content/docs/zh-cn/sdk/documents.md)，本文只记录实施证据。

`EditorState` 仍是唯一可变文本、选择与 Undo/Redo 来源。文档描述只缓存不可变元数据，按 capability revision 复用编码/EOL/字节长度；范围读取借用现有 Rope，仅分配请求范围。`DocumentSession` 负责 dirty 与磁盘保存。原生输入的现有 `Change` 接缝推进版本并调用 `session.note_edit()`，只读替换通过既有 silent replacement 与 `accept_disk_reload` 更新会话，不能变成用户编辑。

`ResourceIdentity::Local` 是普通规范化工作区相对路径，保留 `#`、`%` 字面含义；打开时重新校验 canonical containment。`ResourceIdentity::Virtual` 持有运行时颁发的实例/作用域句柄，原生 Tab 复用同一存活标记，关闭、释放和退役立即撤销。URI 不被解释为本地权限，虚拟 Tab 不进入恢复、文件监听、本地历史、首次使用扫描或旧文件预览入口。

旧 `SubscribeDocuments`/`DocumentChange` 保持字段、最新 revision 合并与本地路径语义。新事件必须显式 `SubscribeDocumentEvents` opt-in，不因旧 SDK 的 `^1` 协商到 `1.1` 而自动发送新通知变体。`WillSave` 只观测保存，不承诺阻塞或拦截。事件入口及每订阅队列各 128 条，溢出产生终止失败；取消/回调内释放后不继续送事件。两种订阅共用每实例八个名额。

比较只保留不可变版本、差异字节范围、自己的 decoration collection 与运行时颁发的只读 `EditorAuthority`。authority 在唯一的 State 请求构造接缝借用现有 `plugin_services.alive` 和完整 `Context.lifetimes`，没有新的存活标记、访客可构造字段或 mutator。它不绑定 transient request 的完成/超时；两份源都是本地文件时也受发起实例与委派链退役约束。左侧借用原有 EditorState 并临时只读；右侧本地文档保持正常编辑。实际左焦点的 Save/SaveAs、格式化与 Rename 经过只读门禁。任一源版本、活动右目标或 authority 变化撤销比较并仅清理自己的标记，恢复左侧原有 readonly 状态；显式 Close 把焦点返回右文档，自动清理保留其他对话框/控件焦点。

新增只读 frame 的 palette、字体和字号映射收拢在 `ui/controls/editor.rs`；比较调用方只布局，主题变化只重着色自己的差异集合。右侧仍沿用已有普通 InputEditor 的渲染，后者会投影上游 editor style。原生 Dark 下两侧 gutter 颜色并不保证相同；本单避免左侧沿用浅色样式，没有扩大为普通编辑器控件迁移。

左右来源有命名的原生 Group，左侧 InputBase 复用现有文本实体、实际 FocusHandle 和惰性的 accessibility value。GPUI 没有公开的输入 readonly flag，故左侧使用具有只读语义的 Document role，且不注册 SetValue；不会把只读文本错误宣布为可写输入。右侧保留正常 InputEditor 接口，关闭比较使用带本地化名称的 Base Button。测试用 TestWindow 不激活 Windows AccessKit，GPUI 只验证同源原生属性与输入行为；实际辅助功能树仍单独验收。

供后单复用：`plugin_document_version`/`plugin_document_info` 与正式 `DocumentVersion` 校验提供身份和当前版本；后台文本仍由目标 Tab 的 EditorState 持有。03 不需要新增可变文档模型；05 可使用独立 decoration owner，不能清除其他特性的集合。

## 实际 SDK 消费者和构建

两个独立 Cargo 项目在初轮 `0.1.0` 后同步升为 `0.1.1`，声明 `editor.documents ^1.1`、`editor.virtual ^1` 和 `editor.diff ^1`。历史包只申请 `editor.read`/`ui.panels`；生成预览包另外申请 `workspace.read`，用于覆盖通用本地打开路径。它们通过清单菜单命令进入正式 SDK，不依赖工单 02 新命令协议或宿主白名单。正式 ZIP 已含管理器固定读取的 `README.md`，分别 1781/1826 字节；没有新增 manifest 的 readme 字段。

```powershell
# 在上述工作树执行；完整包驱动内部调用正式 --plugin-cargo。
& ../target01/debug/editor-app.exe --plugin-package plugins/history-preview --output target/community-api --debug
& ../target01/debug/editor-app.exe --plugin-package plugins/generated-preview --output target/community-api --debug
& ../target01/debug/editor-app.exe --plugin-package plugins/javascript --output target/community-language --debug
```

| 实际包 | SHA256 | 用途 |
| --- | --- | --- |
| `target/community-api/history-preview-0.1.1.zip` | `793af3015f7a8a7aee8668d1c2947380f6cfe75b90693e00434fb17a79c4d25e` | 历史只读内容、事件、刷新与比较 |
| `target/community-api/generated-preview-0.1.1.zip` | `6adddb8976f33fdc059dc7e8c2ec62be3f4fd66495d1259a56a17674ca7a8d2c` | 当前未保存快照派生文本、本地打开与比较 |
| `target/community-legacy/capability-example-0.17.0.zip` | `60a274451122f9b0c820cb202b981e010caaf6339d4bc97cccbe67cfcf4e0f85` | 真正旧 SDK 的本地订阅与编辑/保存回归 |

旧 SDK 包由已有基线宿主 `C:/Projects/RustProjects/Editor/target/debug/editor-app.exe --plugin-package plugins/capability-example --output target/community-legacy --debug` 实际构建，SDK 摘要 `37bd2c7876d625760b0dc1914742de76715de31afe95917cfcc7780493636f9c`。旧宿主仅用于产生旧编码器证据，所有新契约与兼容回归由本候选 Manager/宿主执行。没有恢复旧基础协议。

日志在本工作树忽略的 `target/`：修复候选 `history-review-fix-package.log`、`generated-review-fix-package.log` 分别记录 7.10/7.39 秒，均从新宿主内嵌 SDK 实际构建；初轮另有 `history-accessibility-final-package.log`、`generated-accessibility-final-package.log`、`community-language-package.log`、`document-legacy-sdk-package.log`。已有旧保存回归需要的路径用普通 `Copy-Item` 准备为 `target/plugin-api-test/capability-example.zip`；语言回归的 JavaScript 包复制为 `dist/plugins/javascript.zip`。没有执行历史脚本。

原生复核后，仅将两份 README 的构建占位符换成可执行的仓库默认命令，并以相同宿主重新打包（`history-review-final-package.log` / `generated-review-final-package.log`）。早先 ZIP 为 `8f147…` / `a77fcc…`；最终 ZIP 见上表。归档内 WASM SHA256 前后分别保持 `d2d7e0d53e4eea0f89e6ad4b95174ee06464b88275f6782f26e8c8deb65d5623` / `1d24504aaf87d14f4287632db37729b20e0a62fd19dc02366000cfa20012f214`。宿主、SDK、manifest 与 WASM 行为未改，GPUI/原生证据可复用；最后的 actual runtime 包矩阵使用最终 ZIP。

## 先红后绿与主责矩阵

每个切片从实际 SDK 包进入公开 Manager 开始。最初枚举、详细事件、范围读取和虚拟打开分别在 `document-red.log`、`document-events-red.log`、`document-snapshot-red.log`、`virtual-readonly-red.log` 出现实际 `unknown variant` 的红结果，再实现对应公开入口。只读刷新另发现 silent replacement 不触发 `Change`，补显式版本推进后绿。算法边界先红后绿由比较模块的纯测试记录。

原生首轮发现 URI 被首次使用扫描 canonicalize 后产生 `os error 123`；`virtual-first-use-red.log` 通过真实 SDK 开虚拟 Tab，再调用生产首次使用路径，实际断言有 `InspectBundle`，确认为红。修复按资源身份过滤，同时补旧 FileContext 与提供者选择门禁。主题首轮失败与 `9d645…` 候选复核见[原生记录](native-01.md)，后者已观察左 gutter/current line 深色同步及扫描错误消失。

原生继续发现双栏没有基本辅助语义，`community-a11y-red.log` 通过同源原生 accessibility 属性实际失败，再加入本地只读 frame/来源/按钮名称，最终比较矩阵通过。`community-sdk-docs-red.log` 实际失败于 `README.mddocuments/`、缺少主标题；补路由映射与双语标题后 SDK 导出七例通过。新政策锚点也揭示站点测试把 fragment 当页路径；仅修正页解析，保留独立锚点检查，最终站点 17 例通过。

| 主责 | 自动化可观察行为 | 状态 |
| --- | --- | --- |
| T01 | 原生输入的未保存中文/emoji/CRLF 快照与 dirty/编码/EOL/字节长度；正式本地打开保留 `#%20`；完整读取限额及小范围；磁盘仍旧值 | 自动化通过 |
| T02 | native 打开/关闭重开、编辑、Ctrl+S、活动、选区/视口送真实 Manager；顺序递增及 WillSave→DidSave；取消、溢出失败/补读/重新订阅；旧 SDK 订阅可继续使用 | 自动化通过 |
| T03 | UTF-8/UTF-16 与 CRLF/non-BMP 对应；拒绝 surrogate/字符切分、越界、旧 revision、关闭旧 ID、外国虚拟资源、缺权限/应用实例；身份不授予权限 | 自动化通过 |
| T04 | 两个 SDK 消费者菜单完成打开/刷新/定位/比较；两栏实际绘制；鼠标左焦点后真实键盘输入/Save 拒绝，不保存右 dirty；正常只读 Tab 不落临时文件/恢复；虚拟不扫描、不改提供者选择；实际 Close 后右焦点恢复 | 自动化通过；受影响原生复核见 native-01，待双轴复审 |
| T05 | 显式资源释放、native Tab 关闭、插件禁用；失败候选保持旧文本/面板；重新打开产生新 ID；过期比较消失，其他 owner 标记保留；local-owner 替换/禁用/信任撤销清理与原 readonly/他人焦点保留 | 自动化通过；真正控制器 stop 原生复核见 native-01，待双轴复审 |

GPUI 夹具仅适配现有 worker 的发布与事件入口：正式包产生 `EditorRequest`，生产队列分派到真实应用，断言实际 EditorState/磁盘/可见 bounds。没有插件专用宿主测试 API。事件夹具按用户手势逐批送现有 actor 入口；不把停止消费后的 128 条合约溢出当作正常事件丢失。溢出单独在真实 Manager 用例验证。

## 初轮候选交付检查

```powershell
$env:CARGO_TARGET_DIR = 'C:/Users/Tmiracle/.codex/worktrees/plugin-api-community/target01'
cargo test -p plugin-runtime --test community_documents -- --ignored --nocapture
cargo test -p editor-app extensions::community_document_tests:: -- --ignored --nocapture
cargo test -p editor-app typed_editor_requests_read_selection_and_save_without_switching_documents -- --ignored --nocapture
cargo test -p editor-app installed_javascript_formatter_can_be_replaced_without_changing_analysis -- --ignored --nocapture
cargo test -p editor-app editor::comparison:: -- --nocapture
cargo test -p editor-app editor::linked_input:: -- --nocapture
cargo test -p editor-app sdk_export::tests:: -- --nocapture
cargo fmt --check
cargo test --workspace --exclude editor-app
cargo check --workspace
```

| 命令/日志 | 实际结果 |
| --- | --- |
| `cargo fmt --check` / `community-gate-fmt-final.log` | 通过 |
| 非 UI workspace / `community-gate-tests-final.log` | 224 passed，0 failed，185 ignored；这些 ignored 未计为通过 |
| `cargo check --workspace` / `community-gate-check-final.log` | 通过，0.92 秒；单面板夹具收敛后重新执行全部三门禁 |
| 实际 WASM 运行时 / `community-runtime-accessibility-final.log` | 4 passed，0 ignored，91.35 秒 |
| GPUI 实际 SDK 文档矩阵 / `community-ui-accessibility-final.log` | 3 passed，0 ignored，203.83 秒；单进程串行执行，比较场景含 en/zh-CN × light/dark × 1/1.5 的八次原生重绘/布局/名称/值检查 |
| 旧 SDK 保存 / `community-old-sdk-save-final.log` | 1 passed，0 ignored，56.13 秒；覆盖 selection/save、可见性 hide/reopen、磁盘 reload 的 stale revision；前两次布局失败与归因见下文 |
| 实际 JavaScript 语言消费者 / `community-language-regression.log` | 1 passed，0 ignored，15.05 秒；首次使用准备已批准的私有 Node 依赖 |
| 比较算法 / `community-comparison-unit.log`；linked input / `community-linked-regression.log` | 各 2 passed，0 ignored；只读 frame/文档导出追加前执行，所测算法及 linked input 实现未变 |
| SDK 独立导出 / `community-sdk-docs-final.log` | 7 passed，0 ignored，0.26 秒；实际导出页/README链接/稳定政策锚点存在且可跳转 |
| 站点 `npm run build` / `community-website-final.log` | Astro 构建成功；文档/链接/中英文搜索 17 passed，0 skipped |

编译中出现既有未使用方法/导入与 MSVC `LNK4217` Wasmtime/Tree-sitter 链接警告，构建退出成功；没有为消除本单之外警告扩大修改。三项仓库门禁不包含应用 UI 或 ignored 的实际 WASM 测试，以上单独准备并显式运行。

旧 SDK 保存回归的 `ReadSelection`/`SaveDocument` 与磁盘断言前两次均通过，首轮随后失败于 hide welcome 必扩宽。批准基线 `SessionState::default` 已有 `messages_visible=true`，本单未改 messages/session 的生产实现；共享 right dock 仍有消息面板时，不能要求只隐藏 guest 就撤销整个区域。第二轮仅提前隐藏消息，使既有 retained dock 先关闭，后续 show 未绘制 welcome（`community-old-sdk-save-hidden-peer.log`）。最终单面板夹具通过已有 Base `remove_panel` 移除无关消息 leaf 后完整通过；没有为测试改变生产共享布局。首轮原日志保留为 `community-old-sdk-save-initial.log`。

## 双轴初轮发现与修复记录

固定初轮审查范围为批准基线 `452994e…` 到候选 `3a0d34561a3a4d60928ac66185ca3b4b17059429`。根代理随后单独提交原生/状态记录 `c4bddb124d1af53c31b7e049d0e9b22f36a569dd`；该提交没有修复或关闭发现。以下是本次修复，不代表已经通过复审。

| 轴 | 初轮可操作发现 | 修复与可复用证据 |
| --- | --- | --- |
| Spec | P2 两份本地源比较不跟随发起实例退役 | 保留运行时现有 owner/完整委派 lifetimes 的 opaque authority；单个 actual SDK/GPUI 场景验证失败候选保留、成功替换/禁用/信任撤销清理、恢复原 readonly、保留他人 marks/focus |
| Spec | P2 显式 Close 后键盘焦点丢失 | 实际 Base Button callback 获取 Window 并聚焦右侧现有 EditorState；自动失效清理不移动焦点 |
| Spec | P3 SDK 公共政策提前宣称本单尚未提供的能力 | 删除具体能力名单，保留通用稳定/协商/弃用规则；各能力状态在对应页声明 |
| Standards | P2 Close 焦点、P3 公共政策 | 与上述同一实现和证据共享，不重复整套测试 |
| Standards | P3 读者页包含插件源码路径、构建命令与具体菜单 | 双语 documents 页只给通用 SDK 精确版本工作流；两包的 README 承担各自用法，正式 ZIP 打包 README，清单/Cargo 版本同步升为 0.1.1 |
| Standards | P3 新只读外观散落在比较调用方 | palette/字体/字号映射移入本地 readonly_editor，比较只保留布局，普通编辑器外观不扩大迁移 |

`../target01/comparison-close-focus-red.log` 在真实按钮点击后失败于右编辑器 focus，70.90 秒。第一轮修复已通过该断言，但新增的 recompare 会刷新只读全文，Base 将外部 range 从 `0..3` 映射成 `0..33`，旧位置断言随后失败（`comparison-close-focus-green.log`，87.11 秒）。夹具改为记录重比较后的映射范围，再核对清理保留该范围，没有修改生产 range 映射。`../target01/comparison-owner-red.log` 通过 actual generated SDK 发起 local→local 比较，在成功替换后失败于比较仍存活，64.96 秒。

既有服务调用链回归另外检查已完成 native 请求的 authority：完成及过期 deadline 不撤销，原始 caller 与 provider 退役分别撤销；沿用现有服务包/公开 Manager 请求入口，无新的测试宿主 API。最终结果由下方追加记录及根代理复审确认。

## 修复候选检查

本节的代码与 SDK 文档对象为 `95a333…` 宿主；继续使用同一 `target01` 和正式 0.1.1 包。没有将初轮结果当成本次新增行为通过。

| 命令/日志 | 实际结果 |
| --- | --- |
| GPUI actual SDK 文档矩阵 / `target/community-review-fix-ui.log` | 4 passed，0 failed，0 ignored，298.74 秒；关闭焦点与 local-owner 退役矩阵均包含在其中 |
| SDK 独立导出 / `target/community-review-fix-sdk.log` | 7 passed，0 ignored，0.36 秒；DOCUMENTS 页、README 路由及稳定政策锚点实际导出 |
| 真旧 SDK 保存回归 / `target/community-review-fix-old-sdk.log` | 1 passed，0 ignored，45.17 秒；未恢复旧 wire 协议 |
| 实际服务调用链 / `target/community-review-fix-services.log` | 1 passed，0 ignored，251.77 秒；完成/过期 deadline 保留 authority，caller/provider 退役分别撤销，旧有取消/委派权限和资源清理场景仍通过 |
| 最终 ZIP 的 actual runtime 包矩阵 / `target/community-review-fix-runtime.log` | 4 passed，0 ignored，198.68 秒；详细事件、枚举、权限/作用域及真正旧 SDK 的 documents 1.1 本地订阅均通过 |
| `cargo fmt --check` / `target/community-review-fix-fmt.log` | 通过 |
| 非 UI workspace / `target/community-review-fix-workspace-tests.log` | 224 passed，0 failed，185 ignored；ignored 未计为通过 |
| `cargo check --workspace` / `target/community-review-fix-workspace-check.log` | 通过，6.41 秒 |
| 站点 `npm run build` / `target/community-review-fix-website.log` | Astro 构建成功，17 passed，0 skipped；两语内容/链接/锚点/搜索均通过 |

三项仓库门禁、针对性 GPUI/actual WASM/SDK/站点检查均通过。最终原生 Close 和真正 TTY dev stop 已由根代理在同 hash 的副本与 fresh profile 记录通过；修复后双轴复审尚未完成。

## Windows 原生复核与后续

根代理使用 Computer Use/Windows 输入在自有副本与隔离 `--profile` 中验收，日常 Nanobug 窗口未动。首轮已观察：双栏/只读标识、右栏未保存中文 emoji 输入触发比较失效、重新比较读到最新文本、左侧 Ctrl+S 明确拒绝且右侧仍 dirty、磁盘 hash 不变、左侧输入不改变历史文本。首轮同时发现路径扫描错误与左侧主题陈旧，不能据此判定 T04/C03 完成。

`9d645…` 复核确认了主题和路径扫描修复、dirty 输入及只读保存，但继续发现新增比较区域没有基础辅助语义，因此不是最终通过。`fc63…` 随后通过来源/只读值/按钮的实际 UIA 观察与输入/保存门禁，仍在显式 Close 丢失焦点，初轮失败证据保留在 `native-01.md`。本次 `95a333…` 复核确认左侧只读与 UIA 仍成立；Close 后不重新点击编辑器，Ctrl+A 实际选中右文本，输入中文/emoji成功，磁盘 hash 不变。真正 TTY 控制器 `stop` 返回 0，自有进程/窗口消失。第一次遗漏 TTY 导致 stdin 关闭后的自有进程清理不计作正常退出；详见[原生记录](native-01.md)。仍需完成双轴复审、普通提交/推送读回后才关闭 #93。本候选不宣称其他工单或完整平台已验收。
