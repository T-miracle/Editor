# 文档资源与命令交互整合候选

日期：2026-10-09。状态：本地合并候选；仓库门禁执行中，新增连接的双轴审查与原生复核待完成。尚未推送，不修改已交付的 #93 / #94，也不实施 03 / 05。

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
| `cargo fmt --check` | **通过** | `target/community-01-02-fmt.log` |
| `cargo test --workspace --exclude editor-app` | 执行中 | `target/community-01-02-workspace-tests.log` |
| `cargo check --workspace` | 接上述门禁顺序执行 | `target/community-01-02-workspace-check.log` |
| `npm run build`；`npm test`（`website`） | **均通过；17 passed / 0 failed / 0 skipped**，含实际搜索索引 | `target/community-01-02-website-build.log`、`target/community-01-02-website-tests.log` |
| `git diff --check` | **通过** | 仓库普通配置；CRLF 转换提示不计失败 |

新 GPUI 两用例验证虚拟 `.js` 标题却声明 Rust、后台捕获后切到本地 Tab、真实 guest 的虚拟上下文、刷新拒绝旧回调、关闭后 URI 不成为磁盘目标；左栏实选区、右键与 Base 行点击，guest 收到左侧 Rust / `has_selection=true` / `writable=false` / `path=None`，右 buffer 保持。菜单仍打开时 SDK 刷新使比较退役，无再次点击执行 Ctrl+A 和中文 emoji 输入，只改右 buffer，磁盘原文不变。仅复跑合并接缝，不复制两单已交付的完整 negative 与原生矩阵，不把 skipped / ignored 算作通过。

四个实际 SDK 运行时子用例分别按以下精确过滤运行，全部显式执行 ignored：

```powershell
cargo test -p plugin-runtime --test host_interaction typed_commands_validate_values_and_preserve_the_original_call_authority -- --ignored
cargo test -p plugin-runtime --test selected_resources direct_host_typed_command_selects_reads_and_releases_its_own_resource -- --ignored
cargo test -p plugin-runtime --test plugin_services delegated_resources_and_async_continuations_follow_the_original_source_lifetime -- --ignored
cargo test -p plugin-runtime --test community_documents document_reader_can_opt_in_to_ordered_events -- --ignored
```

## SDK、产物与原生准备

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

最终宿主绝对路径为 `C:/Users/Tmiracle/.codex/worktrees/plugin-api-community/target01/debug/editor-app.exe`，末轮构建后补充摘要。最后关闭 URI 的防探测 guard 仅影响 native app，不改变嵌入 SDK 或五包，可以复用上述产物。

## Standards、Spec 与原生复核

固定审查范围为 `b2f20def89d895d17d54138f80ba7a47da812ae3` 至本次普通 merge 候选；两轴待主线程安排。两单自己的交付结论不代表新增接线已审查通过。

新增物理范围仅为左栏菜单实际点击、可见 context，以及菜单有焦点时开发实例 reload 退役比较后，无 reclick 的右输入与正常 stop。主线程准备自有 `C:/Users/Tmiracle/.codex/worktrees/plugin-api-community/native-qa-integration/` 的旁置插件、profile 和 workspace，按公开 shipped_roots / Manager 入口运行。本代理没有操作桌面；原生复核及最终推送待主线程，不用本次 GPUI 代替物理输入。
