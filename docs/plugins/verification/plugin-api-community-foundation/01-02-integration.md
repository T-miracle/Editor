# 文档资源与命令交互整合候选

日期：2026-10-09。状态：本地合并候选的两项初审发现已修正，最终仓库门禁与针对性回归通过；独立复审和新增原生复核待主线程完成。尚未推送，不修改已交付的 #93 / #94，也不实施 03 / 05。

## 固定范围与冲突决策

从 01 集成 HEAD `b2f20def89d895d17d54138f80ba7a47da812ae3` 普通合并 `codex/plugin-api-interaction` 的交付点 `8506cc8f1f9baee3fca030d59b7d1d21b5227763`，保留双方历史，不 cherry-pick 或重写。两单详测分别见 [01](01-documents-and-virtual-resources.md)、[02](02-commands-and-interaction.md)；未改变的物理输入按 [native-01](native-01.md)、[native-02](native-02.md) 明确范围复用。

十处冲突按同一契约整合：协议枚举与 SDK 导出同时保留 documents / virtual / diff 和 commands / interaction；运行时同时保留不透明 `EditorAuthority`、类型化命令、选择资源和原生等待取消。持久比较仅观察发起实例及原始调用链的存活，不绑定已经完成的请求或瞬时等待期限。虚拟和选择句柄分别校验归属，不能相互替换为工作区文件授权。

菜单目标区分本地路径与精确文档版本。虚拟 Tab 在任何 filesystem 操作前解析；已关闭或未知资源 URI 也不进行 `canonicalize` / `is_dir`。语言、选择、只读性来自被点击文档；虚拟目标为 `path=None`、`extension=None`、`directory=false`、`writable=false`。后台捕获不随活动 Tab 改变，过期版本和实例 incarnation 拒绝执行。

左比较栏的实际右键复用本地 `PopupMenu` 的 Base 行为、外观和键盘能力，回调捕获左侧目标。失效、替换、显式关闭均先正常 dismiss 菜单再撤比较；只有菜单拥有焦点才恢复来源，再复用既有的选择性焦点修复，避免恢复到卸载的左栏或抢其他对话框焦点。

两份 01 消费者适配 `Notification::Command` 新增的可选 `context` 字段，参数探针仍只读取显式参数；清单、Cargo 和锁文件同步从 0.1.1 升为 **0.1.2**。双语命令页明确虚拟和比较目标，共用稳定政策；没有插件 ID 专属宿主分支或测试专用 API。

## TDD 与验证

工作目录 `C:/Users/Tmiracle/.codex/worktrees/plugin-api-community/Editor`；所有 Cargo 使用 `CARGO_TARGET_DIR=C:/Users/Tmiracle/.codex/worktrees/plugin-api-community/target01`。Rust / Cargo 1.98.1、Windows MSVC、现有 `wasm32-wasip2`；未安装或升级工具链。实际包经公共 Manager、生产 worker publication、宿主请求路由进入原生编辑区。

| 执行 | 实际结果 | 日志（相对工作目录） |
| --- | --- | --- |
| 虚拟上下文 red | 0 passed / 1 failed；声明 `rust` 却得到 `text`；78.84 秒 | `target/community-01-02-menu-red.log` |
| 左侧实际菜单 red | 1 passed / 1 failed；虚拟捕获已绿，左栏无可见行；147.32 秒 | `target/community-01-02-menu-left-red.log` |
| `cargo test -p editor-app community_document_tests::menus:: -- --ignored --test-threads=1` | **2 passed / 0 failed / 0 ignored**；218.74 秒 | `target/community-01-02-menu-green.log` |
| `cargo test -p editor-app native_plugin_menus_revalidate_context_and_remove_retired_contributions -- --ignored --test-threads=1` | **1 passed / 0 failed / 0 ignored**；69.41 秒 | `target/community-01-02-native-menu-test.log` |
| `cargo test -p editor-app sdk_export::tests:: -- --nocapture` | **7 passed / 0 failed / 0 ignored** | `target/community-01-02-sdk-tests.log` |
| `cargo test -p editor-app ui::controls::menu::tests:: -- --nocapture` | **4 passed / 0 failed / 0 ignored** | `target/community-01-02-popup-tests.log` |
| actual SDK 类型化命令原始调用授权子用例 | **1 passed / 0 failed / 0 ignored**；124.54 秒 | `target/community-01-02-runtime-commands.log` |
| actual SDK 直接宿主命令选择、读取、释放子用例 | **1 passed / 0 failed / 0 ignored**；64.60 秒 | `target/community-01-02-runtime-selection.log` |
| actual SDK 委托资源及异步延续生命周期子用例 | **1 passed / 0 failed / 0 ignored**；227.52 秒 | `target/community-01-02-runtime-lifetime.log` |
| actual SDK 文档有序事件子用例 | **1 passed / 0 failed / 0 ignored**；38.05 秒 | `target/community-01-02-runtime-documents.log` |
| `cargo fmt --check` | **通过**；最终候选再检查退出 0 | `target/community-01-02-fmt.log`、`target/community-01-02-fmt-final.log` |
| `cargo test --workspace --exclude editor-app` | 首轮链接失败，退出 101；现有 MSVC 环境初始化后 **230 passed / 0 failed / 203 ignored**，62 个结果批次 | `target/community-01-02-workspace-tests.log`（保留失败）、`target/community-01-02-workspace-tests-final.log` |
| `cargo check --workspace` | **通过**，退出 0；20.83 秒 | `target/community-01-02-workspace-check.log` |
| `npm run build`；`npm test`（`website`） | **均通过；17 passed / 0 failed / 0 skipped**，含实际搜索索引 | `target/community-01-02-website-build.log`、`target/community-01-02-website-tests.log` |
| `git diff --check` | **通过** | 仓库普通配置；CRLF 转换提示不计失败 |

新 GPUI 两用例验证虚拟 `.js` 标题却声明 Rust、后台捕获后切到本地 Tab、真实 guest 的虚拟上下文、刷新拒绝旧回调、关闭后 URI 不成为磁盘目标；左栏实选区、右键与 Base 行点击，guest 收到左侧 Rust / `has_selection=true` / `writable=false` / `path=None`，右 buffer 保持。菜单仍打开时 SDK 刷新使比较退役，无再次点击执行 Ctrl+A 和中文 emoji 输入，只改右 buffer，磁盘原文不变。仅复跑合并接缝，不复制两单已交付的完整 negative 与原生矩阵，不把 skipped / ignored 算作通过。

首轮完整 workspace 在链接 `plugin-protocol` lib test 和 `plugin-runtime` 的 `code_highlighting` 时出现 `LNK1104: msvcrt.lib`，尚未完成该轮测试，也未继续 check/build。主代理只读核对实际 x64 库存在、当前 `LIB/LIBPATH` 未初始化，使用已安装 Visual Studio 的 `Enter-VsDevShell` 为当前构建进程设置 MSVC / Windows SDK 搜索路径，再运行原命令；没有安装工具、修改全局环境或更改源码。初轮失败不计作通过。

```powershell
Import-Module 'C:/Program Files/Microsoft Visual Studio/18/Community/Common7/Tools/Microsoft.VisualStudio.DevShell.dll'
Enter-VsDevShell -VsInstallPath 'C:/Program Files/Microsoft Visual Studio/18/Community' -Arch x64 -HostArch x64 -SkipAutomaticLocation
```

四个实际 SDK 运行时子用例分别按以下精确过滤运行，全部显式执行 ignored：

```powershell
cargo test -p plugin-runtime --test host_interaction typed_commands_validate_values_and_preserve_the_original_call_authority -- --ignored
cargo test -p plugin-runtime --test selected_resources direct_host_typed_command_selects_reads_and_releases_its_own_resource -- --ignored
cargo test -p plugin-runtime --test plugin_services delegated_resources_and_async_continuations_follow_the_original_source_lifetime -- --ignored
cargo test -p plugin-runtime --test community_documents document_reader_can_opt_in_to_ordered_events -- --ignored
```

## 首轮 SDK、产物与原生准备

新宿主通过以下正式入口一次重建五包：

```powershell
& '../target01/debug/editor-app.exe' --export-plugin-sdk target/community-01-02-sdk
& '../target01/debug/editor-app.exe' --plugin-package plugins/history-preview plugins/generated-preview plugins/capability-example plugins/example plugins/terminal --output target/community-integration --debug
```

五包 Cargo 消息均指向 SDK key **`244dd7e20f5844e6a77273b76ebbd10f90efc2b1fa3b74bff07522aa89f9a105`**，日志 `target/community-01-02-packages.log`，缓存位于 `C:/Users/Tmiracle/AppData/Local/MeEditor/plugin-sdk/<key>`。本轮是正式 ZIP / debug WASM，未宣称 Release。七项 SDK 导出回归包含三页本地链接和公共政策锚点。

以下 ZIP 的绝对目录为 `C:/Users/Tmiracle/.codex/worktrees/plugin-api-community/Editor/target/community-integration/`。

| ZIP | ZIP SHA-256 | 包内 WASM SHA-256 |
| --- | --- | --- |
| `history-preview-0.1.2.zip` | `c372238d112502950e6829523ee621d06d7d5d4e9e4d7362f1a07958535b3117` | `02e46920ca3521302644ef7d35d0036f93378bbbd2976ebe24e70ab257cbdeb6` |
| `generated-preview-0.1.2.zip` | `d3052f140e9b1b8e3203738b152f327cf3805571225014a9eddb34e06d85f868` | `e45ab007e5a041d11ca0d23f11d2d334b6bd2cfc4dd5f9d441d34828ab579077` |
| `capability-example-0.18.0.zip` | `7b2668b34e69b49d5dfc9e2c26752dfad9a8a11a1c50b7d2edcfc349819eca23` | `9aaa0933da6a949868d60635dab713fc9f63b80be7f27649cbeb754d15288639` |
| `example-0.4.0.zip` | `c04645c5ed76a2c3a81b04b9f65c5b00ca9cadc15ba288d674763b5d1302d1d9` | `e8f4d62570d1aeba1fc79b5e9ed71150049b36680adec8ff78d75af528c23353` |
| `terminal-0.12.3.zip` | `c4d574ff4eac6daf5450efcab833f4d52b8001cc3a2b44bf1295b7e48ffb33aa` | `fd298270cc48a3da516cc68f53c5ac29faa25e88a170af2137d052cddd23d5bc` |

同目录的原生辅助 `document-menu-reader-0.18.0.zip` SHA-256 为 `8c1c1fd90dbfa77cc259b9514791de47b707256fc48b51be632ecd362fccc9d9`。与测试相同 manifest 增量：独立 ID 加只读条件菜单；WASM 仍为上表 capability-example 的原字节，仅直接归档，不重复编译或修改访客实现，仍经普通包验证与 Manager 安装。

首轮固定 `ef6fa2d09948871dac95ead9c42fddab76623e45` 的宿主由主线程在同一 MSVC 环境下构建通过（39.40 秒；`target/community-01-02-build-final.log`），SHA-256 为 `ab9a0f614159bfc13052c74c432120c0a71627d5800f70e29a28109a809fb097`。主线程仅观察初始 Ready，未将其记作新增菜单原生验收。下节的新宿主和 SDK 已替代这些首轮产物，不以旧 key 代表修正后的 reader asset。

## Standards、Spec 与原生复核

固定初审范围为 `b2f20def89d895d17d54138f80ba7a47da812ae3` 至普通 merge `ef6fa2d09948871dac95ead9c42fddab76623e45`。Standards 发现 1 项 P3（坏味道 0 项），Spec 发现 1 项 P2；修正及回归见下节。修正尚待独立复审，两单自己的交付结论不代表新增接线已审查通过。

新增物理范围仅为左栏菜单实际点击、可见 context，以及菜单有焦点时开发实例 reload 退役比较后，无 reclick 的右输入与正常 stop。主线程准备自有 `C:/Users/Tmiracle/.codex/worktrees/plugin-api-community/native-qa-integration/` 的旁置插件、profile 和 workspace，按公开 shipped_roots / Manager 入口运行。本代理没有操作桌面；原生复核及最终推送待主线程，不用本次 GPUI 代替物理输入。

## 整合初审修正与最终回归

Spec P2：比较左侧原先把 `DocumentVersion.path` 当作本地路径解析；该字段对于本地文档是工作区相对名，进程目录的同名文件会提供错误上下文，缺少同名文件则菜单为空。修正只接收完整 `DocumentVersion`，精确匹配现存 Tab 的身份、路径和 revision，再取该 Tab 的本地绝对路径供描述性元数据使用；虚拟资源仍无本地路径。过期版本保留原始文档目标并由既有调用入口拒绝，不能回退到磁盘推测身份，也不新增权限来源。

新增 actual SDK / GPUI 子用例先在旧生产入口取得 red：临时工作区左文件命名为 `Cargo.toml`，与进程目录的同名文件碰撞；真实右键及 Base 行点击没有把左上下文交给 guest，JSON 回执解析失败（**0 passed / 1 failed / 0 ignored，118.10 秒**；`target/community-01-02-local-menu-red.log`）。此前测试准备时误用不存在的 snapshot 方法产生 E0599，已改用既有 Base snapshots；该编译失败单独保留于 `target/community-01-02-local-menu-compile-initial.log`，不计作行为 red。

绿测沿公共 Manager 和 guest `CompareDocuments` 比较两个本地文档，实际选中左内容、右键、点击原生菜单行，断言来自左文档的语言、工作区相对路径、扩展名、选择、只读和非目录上下文；无论右侧活动文档还是 cwd 同名文件均不成为目标。随后通过实际关闭按钮退出比较，原生输入修改左 session，重新比较恢复相同只读条件，再重放旧 captured callback，验证拒绝原因是 revision 过期；左右磁盘原文与右 buffer 均保持。

Standards P3：英文选择建议规则改为禁止 slash、backslash、colon 和 NUL，且名字**不能等于** `.` 或 `..`；普通含点名字可用，与中文的“不得等于”语义一致。读者正文只改 `website/`；未复制到维护者文档。

最终生产源码的仓库门禁已经收敛；同一 VS DevShell 初始化用于 native build、测试与 check。针对性命令使用本节新 SDK 和正式包，显式执行 ignored；不会将非 UI workspace 中保留的 203 个 ignored 报作通过。

| 执行 | 实际结果 | 日志（相对工作目录） |
| --- | --- | --- |
| `cargo fmt --check` | **通过**，退出 0 | `target/community-01-02-review-fmt.log` |
| `cargo test --workspace --exclude editor-app` | **230 passed / 0 failed / 203 ignored**，62 个结果批次；退出 0 | `target/community-01-02-review-workspace-tests.log` |
| `cargo check --workspace` | **通过**，退出 0；4.19 秒 | `target/community-01-02-review-workspace-check.log` |
| `cargo build -p editor-app` | **通过**，退出 0；30.77 秒 | `target/community-01-02-review-build.log` |
| `cargo test -p editor-app community_document_tests::menus:: -- --ignored --test-threads=1` | **3 passed / 0 failed / 0 ignored**；372.69 秒 | `target/community-01-02-review-menu-green.log` |
| `cargo test -p editor-app ui::controls::menu::tests:: -- --nocapture` | **4 passed / 0 failed / 0 ignored**；0.26 秒 | `target/community-01-02-review-popup.log` |
| `cargo test -p editor-app sdk_export::tests:: -- --nocapture` | **7 passed / 0 failed / 0 ignored**；0.45 秒 | `target/community-01-02-review-sdk-tests.log` |
| `npm run build`（`website`，含站点测试） | **通过；17 passed / 0 failed / 0 skipped**，含实际搜索索引 | `target/community-01-02-review-website.log` |

旧、新正式 SDK 导出的 **60 个文件逐一比较，仅 `INTERACTION.md` 不同**；Rust 代码、WIT 与 Cargo 清单一致。本次生产变化仅为左比较菜单目标绑定及英文 reader asset，先前四项 Runtime actual SDK 权限 / 调用链详测按上文固定结果复用；受影响的本地 / 虚拟菜单、菜单焦点退役、SDK 导出和站点重新执行，不复制无关矩阵。

## 修正候选的 SDK 与原生产物

新宿主通过以下正式入口一次重建五包，版本保持未发布整合候选的 0.1.2；没有修改消费者 Rust 源码。包版本、能力和协议声明均按清单核对。

```powershell
& '../target01/debug/editor-app.exe' --export-plugin-sdk target/community-01-02-review-sdk
& '../target01/debug/editor-app.exe' --plugin-package plugins/history-preview plugins/generated-preview plugins/capability-example plugins/example plugins/terminal --output target/community-integration-review --debug
```

当前 SDK key 为 **`34b1ef263fbab5727e7d145de7b16d710a2fd5d9b093750b36a5e6355b4a0ead`**，五包的实际 Cargo 日志均指向该 key（`target/community-01-02-review-packages.log`）。新的英文规则存在于导出 `INTERACTION.md` 中；不把旧 `244dd7...` 缓存声明为新 key。debug WASM 在新的缓存路径下重新编译，以下记录实际字节，不声称与旧 WASM 等价。

新 ZIP 绝对目录：`C:/Users/Tmiracle/.codex/worktrees/plugin-api-community/Editor/target/community-integration-review/`。

| ZIP | ZIP SHA-256 | 包内 WASM SHA-256 |
| --- | --- | --- |
| `history-preview-0.1.2.zip` | `5ff8035ea4182a5fb4f7ba6e9845f8094a2d9969fd26f66ae15f7a02b7f1d150` | `6e75dd59be5d0e2dfad3723ecce4ae4b211fd1c8e3fee32d99f495252aa2d8cf` |
| `generated-preview-0.1.2.zip` | `105e42c26a564a4e354c7f725b812c9e9dac1835c64e204c5d2946c761410194` | `eb5bd0bdba556e61d326019c8eb5ef503572f8897548977d17ed2f47fdecba78` |
| `capability-example-0.18.0.zip` | `2906091c178228c68ba547321c17244d801bdbcc0c7b0920224cdb5dbf73a6ca` | `66c6492f4b860b4b89e0e439b9ef4c8c42e214af64dab52e22b10f42b2f1f557` |
| `example-0.4.0.zip` | `b6f2b1287f45175bed1882ad434b20fbf2e1a10acad621b3b7aa217b886ae646` | `63ba96162780b06f7f70d51e36f532b9b1b6278bfe52eb1086271dfc8475bb80` |
| `terminal-0.12.3.zip` | `6842de87568c23e0094139015e05d4f178f2efa5e69ba6bbc8825ad1a41a6c8b` | `fcca2ea620746ffade1493758c3ab7226b4f726041fc067a964eaa7e17891665` |

同目录 `document-menu-reader-0.18.0.zip` SHA-256 为 **`4f04224a5eeabb6c4092a18bf7bf7de44fd5eef788cc3083675f691f005dd148`**，仅在新版 capability-example 的 manifest 加独立 ID 与只读菜单，直接归档；其 WASM SHA-256 与上表 capability-example 相同。新静态开发项目位于 `C:/Users/Tmiracle/.codex/worktrees/plugin-api-community/Editor/target/community-integration-native/history-project/`，资源来自新版 history ZIP，保留公开 `nanobug-plugin.json` 入口与预编译 WASM；可通过开发 reload 触发 owner 退役，无需再编译访客。

新宿主绝对路径：`C:/Users/Tmiracle/.codex/worktrees/plugin-api-community/target01/debug/editor-app.exe`；SHA-256 **`ee95fa6057a1c50f29dfdcaebe92e362ddd645ddfee8224568984390ee971e4e`**。新增原生连接与两轴修正复审仍由主线程执行、另记结果；本代理未操作桌面，未推送或修改 issue。

## 主线程收尾：双轴复审与原生连接

生产代码固定候选为 `325c9d701ac46f2755349ab8f22cdce294efd8ef`，在普通 merge 历史上保留修正；随后 `91b7e092ffae3747ea26285a48bfcff0cc3b4914` 仅配对澄清两语 reader 文档。新增原生实际结果见[主代理原生记录](native-integration.md)：真实左栏选区、右键与行点击返回只读虚拟目标上下文；菜单 Down 导航后保持焦点，reload 退役撤下比较/虚拟标签/菜单；无需再次点击编辑区，Ctrl-A 和中文非 BMP 输入只修改右侧未保存文档。控制器正常 stop 退出 0，自有进程为 0，磁盘原始摘要保持。

### Standards

初审 `b2f20def...ef6fa2d` 的 **1 项 P3 硬性规范、0 项坏味道**保留为历史；英文名称规则语义问题已修正。独立复审 `ef6fa2d...325c9d7` 未发现代码规范或坏味道问题，但发现 **1 项新的流程 P3**：英文修正没有与中文在同次提交配对。普通双语提交 `91b7e09` 同时更新两语页面摘要并澄清中文“名称不能等于”规则；没有重写先前提交。独立窄复审 `325c9d7...91b7e09` 确认流程 P3 已解决，**新增硬性违规 0、判断性坏味道 0**。

### Spec

初审 `b2f20def...ef6fa2d` 的 **1 项 P2**保留为已修正历史。独立复审 `ef6fa2d...325c9d7` 确认完整 `DocumentVersion` 精确匹配 live Tab，本地绝对路径仅作描述元数据；虚拟目标无磁盘路径，过期保留原版本并由正式入口拒绝。新增缺失、部分实现、范围蔓延和错误实现均为 **0 项**。后续双语提交未改变产品行为。

两轴最终未解决发现分别为 **Standards 0（坏味道 0）、Spec 0**，各轴无最严重问题。两单既有关闭状态未重开或改写，父设计 #92 未修改。

### 配对文档检查与产物复用

`91b7e09` 仅修改双语页面 description 与等价中文用语，Rust、WIT、访客、权限和 UI 源码没有变化。`npm run build` 重新通过 **17 passed / 0 failed / 0 skipped**（`target/community-01-02-paired-website.log`）；SDK 导出回归重新通过 **7 passed / 0 failed / 0 ignored，0.41 秒**（`target/community-01-02-paired-sdk-tests.log`）；`git diff --check` 通过。文档变更依仓库规则不重复无关 Rust 全量矩阵，生产实现阶段三门禁沿上节真实结果。

宿主重新构建退出 0（`target/community-01-02-paired-build.log`），当前 EXE SHA256 **`5f9a059bd3f937fed92901d61a2309b69d7cb2462b65003363746927a3e10709`**。再次用公开 `--export-plugin-sdk target/community-01-02-paired-sdk` 导出，与 `target/community-01-02-review-sdk` 的 **60 个文件逐一 SHA256 比较，0 个变化**。English frontmatter 被现有 exporter 剥离，中文不进入英文 SDK；因此内容寻址 key 仍为 **`34b1ef263fbab5727e7d145de7b16d710a2fd5d9b093750b36a5e6355b4a0ead`**，五包和静态 history/menu WASM 复用上节准确字节，不重复编译。

原生记录使用实际运行的 EE95 副本，不把新 5F9A 的嵌入站点元数据变化说成另一次桌面运行。新增交互生产源、导出 SDK、插件清单、WASM、工具链与参数均未变化；复用该实际连接证据。此整合点已满足后续 03、05 的公开接线依赖，主线程将正常推送并核对远端后继续后单。
