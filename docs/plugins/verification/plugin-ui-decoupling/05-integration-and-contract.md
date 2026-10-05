# 工单 05：最终契约与集成验收

日期：2026-10-06。议题：[#66](https://github.com/T-miracle/Editor/issues/66)。
分支：`codex/plugin-ui-decoupling`；固定审查起点：`4d4a7b6`。
状态：实现、最终包、事务、权限、仓库基线及总验收通过，等待固定提交双轴审查；未关闭议题。

## 最终边界

移除 `editor.presentation`、`Panel.view_modes`、`PreviewMode`／`PreviewModes`、
`Installed::preview_mode_icon` 与 `Command.toolbar`／`toolbar_icon`。
Panel／Command 严格拒绝未知字段，旧 required 能力和旧字段在安装前提示更新 SDK。
基础协议 7、API base 1 与 UI 文档 1 继续使用，具体能力独立协商，SDK 以内容摘要区分。
历史 session 记录只作为有限不透明数据导入，不保留旧协议执行路径。

插件通过 `editor.layout`、`ui.tools`、`ui.content_colors` 与私有偏好提供域内行为；
宿主保留文件 Tab、唯一原生编辑会话、提供者选择、统一控件及权限／资源绘制。
通用窗口命令菜单、隐藏按钮与预览传输上限提示已使用中英文资源。
图标安全验证沿用最终工具接口，覆盖安装后篡改、64 KiB 上限和真实 Windows junction 越界。

独立布局示例 0.3.0 增加未贡献菜单或按钮的显式 `diagnostic-trap` 命令，用于实际 WASM 故障验收。
它在插件内部触发故障，宿主没有该身份或该命令的专属实现。
原生测试确认隐藏源码后故障恢复同一文本、未保存修改及 Undo／Redo；
PNG 故障保留 Tab、撤销图片资源，实际“重试”按钮经标准生命周期操作恢复预览，卸载清理资源。
停止的诊断记录可保留，公开实例标识必须撤销；测试不以内部容器是否删掉记录代替运行状态。

## U01–U20 与 Image 覆盖矩阵

既有证据对应 [01](01-image-file-views.md)、[02](02-composable-layouts.md)、
[03](03-tools-and-preferences.md)、[04](04-plugin-migration.md) 的明确交付提交。
未受本单旧字段移除影响的行为复用这些证据；最终包与新故障组合按下面记录显式重跑。

| 场景 | 有效证据及本单复核 |
| --- | --- |
| U01 文件 Tab | 01 的实际文本／SVG／PNG 文件流程；本单故障后 PNG Tab 仍可见，无插件 Tab |
| U02 非文本路径 | 01 的 Image 原生显示与文本保存边界；最终 `file_views` 的 FileContext 权限／资源回归 |
| U03 组合布局 | 02 的不同身份独立包与并排／上下／仅内容；最终 `real_layout_packages` 与 `composable_layouts` |
| U04 文本与历史 | 02 的选择、未保存文本、IME 与 Undo／Redo；本单隐藏源码后的真实故障恢复 |
| U05 提供者 | 02 的显式选择、重启与工作区隔离；最终独立包选择、安装不覆盖与失效恢复 |
| U06 布局权／辅助工具 | 02–03 的 auxiliary 拒绝整体布局、非选提供者撤销和辅助功能；最终布局／工具包组合 |
| U07 共享意图 | 03 的两个文件、CAS 与 watch；04 的 Markdown 两个后缀归一，最终实际工具包 |
| U08 有限迁移 | 04 的旧记录导入、重复／冲突保留与重启；最终真实 Markdown／Image 迁移组合 |
| U09 窗口组 | 03 的两组原生显隐与独立溢出；04 的终端显隐，最终 Terminal 迁移原生用例 |
| U10 功能组 | 04 的 Markdown／Image 模式、同步／格式栏及终端功能；最终迁移与独立工具包 |
| U11 目标／溢出 | 03 的捕获文件／窗口目标、键盘及两组 More；最终实际工具包与拒绝迟到事件 |
| U12 图标／主题 | 04 的包内资源、域内 RGB 与用户主题优先；本单实际工具 SVG／junction 与深浅主题组合 |
| U13 文本故障 | 本单实际 WASM trap 后普通编辑、原 Undo／Redo 与未保存文本；02 的手动普通编辑恢复 |
| U14 非文本故障 | 本单实际 PNG trap 后 Tab、原因及原生重试；02 的查看器更换且菜单无强制文本恢复 |
| U15 迟到结果 | 03–04 的版本／目标／提供者／实例门禁；04 原生旧排队定位先红后绿，最终包事件回归 |
| U16 生命周期 | 本单实际故障／重试／卸载；最终准备／激活／提交失败及成功迁移、资源归属回归 |
| U17 权限／信任 | 03–04 当前读授权重新检查与跨实例资源；最终受限工作区、私有句柄与图片／图标越界回归 |
| U18 原生交互 | 01–04 的键盘、中文 IME、焦点、滚轮、缩放、主题与 DPI；最终独立布局、工具和实际插件组合 |
| U19 响应性 | 04 最终 500,008 字节 Markdown 持有旧只读树时输入 377.33 ms；本单最终 SDK 包原生输入 373.50 ms，旧场景无事件权且最终版本精确匹配 |
| U20 既有功能／SDK | 04 实际 Markdown／Image／Terminal／示例与语言回归；本单全包重建、仓库外 SDK 构建与公开 Manager 安装 |
| I01 编辑／只读 | 01 SVG 未保存预览与其他图片无文本能力；最终 Image 真实包与原生 SVG／PNG 迁移 |
| I02 原尺寸／缩小 | 01 OriginalContain 的 PNG／JPEG／GIF／WebP 和真实可见尺寸；最终 Image 与布局实际包 |
| I03 尺寸响应／手动缩放 | 01 原尺寸自动更新与区域缩放；04 同 SVG 编辑保留手动缩放、首帧损坏新文件及同文件损坏恢复回归 |

## 命令与结果

旧契约公开 Package 拒绝先红后绿；工具图标实际包与 junction 读取、本单真实故障原生组合均通过。
最终 SDK 内容摘要为 `a27fedbf8b386899f192ac7ebd8940f283e66bce6d9e954a5c573941dfa6f6d4`。
宿主构建、SDK 导出、损坏缓存修复、仓库外独立组件构建和全部 8 个发行插件包重建通过；
独立布局／工具包已再次构建，Layout Example 0.3.0 包包含本单诊断命令及对应 README。

| 最终检查 | 实际结果 |
| --- | --- |
| `cargo fmt --check` 与 `git diff --check` | 通过 |
| 非 UI workspace | 初始 126 passed／119 ignored；补充旧注册表回归后 128 passed／120 ignored，不把跳过用例计作通过 |
| `cargo check --workspace` | 通过；既有 unused／Wasmtime 链接提示没有新增失败 |
| 应用完整串行基线 | 249 passed，107 ignored；实际 WASM 包对应原生用例单独显式执行 |
| 实际 SDK 包运行时组合 | 13 passed、0 ignored：布局 1、文件 1、工具 5、SDK 包 1、终端 1、图标 1、迁移插件 3 |
| 私有数据更新与失败恢复 | 2 passed、0 ignored：成功升级及迁移／激活／提交失败保留原数据 |
| 资源归属与信任组合 | 3 passed、0 ignored：跨实例及退役句柄、工作区关闭／受限访问、更新失败／撤销信任 |
| 独立布局及真实故障 | 2 项各自通过、0 ignored：完整布局／提供者／IME／Image 重绑定；实际 WASM trap 的文本历史及 PNG 重试 |
| 原生两组底栏 | 1 passed、0 ignored：实际窗口／文件目标、共享意图、键盘、独立溢出、主题及 DPI |
| 真实插件原生迁移 | 4 passed、0 ignored：Markdown 旧偏好及重启、Image 的 SVG／PNG 边界、Terminal 显隐／功能、持续输入 |
| 实际 UI 包与语言切换 | 各 1 passed、0 ignored：示例／Image 原生输入与清理、语言提供者选择／替换／普通回退 |
| 旧 UI 安装记录 | 2 passed、0 ignored：启动／重开／启用拒绝／当前包更新、设置与私有数据／授权保留，以及未知字段仍拒绝 |
| 真实 SDK 安装升级 | 1 passed、0 ignored：旧元数据阻止组件运行，当前合法包重新安装后恢复实例并保留私有文件与用户设置 |
| 历史安装目录迁移 | 1 passed、0 ignored：旧协议含 null 工具字段，两份工作区、原始备份及旧私有数据可恢复；93.24 秒 |

上述检查与 9 项实际包原生场景均有有效通过结果，完整基线中的 ignored 不计为通过。
日志在隔离工作区 `target/final-*.log`，不提交二进制和完整输出。

```powershell
cargo build -p editor-app
./scripts/build-plugins.ps1 -Packages terminal,example,svg,rust,toml,html,javascript,markdown -HostExe ./target/debug/editor-app.exe
./scripts/build-layout-example.ps1 -HostExe ./target/debug/editor-app.exe
./scripts/verify-plugin-sdk.ps1 -HostExe ./target/debug/editor-app.exe
cargo test -p plugin-runtime --test retired_ui_contract
cargo test -p plugin-runtime --test registry_ui_migration --test retired_ui_contract
cargo test -p plugin-runtime --test registry_ui_migration retired_ui_install_updates_through_sdk_and_preserves_private_data -- --ignored --test-threads=1
cargo test -p plugin-runtime --test installed_migration -- --ignored --test-threads=1
cargo test -p plugin-runtime --test sdk_distribution --test composable_layouts --test file_views --test ui_migrations --test tool_artwork --test plugin_tools --test terminal_migration -- --ignored --test-threads=1
cargo test -p plugin-runtime --test data_migration upgrades_private_data_on_an_isolated_copy -- --ignored --test-threads=1
cargo test -p plugin-runtime --test data_migration failed_migration_activation_and_commit_preserve_old_data -- --ignored --test-threads=1
cargo test -p plugin-runtime --test scoped_instances resource_handles_reject_foreign_and_retired_owners_and_release_all_views -- --ignored --test-threads=1
cargo test -p plugin-runtime --test scoped_instances application_instance_survives_workspace_closure_and_restricted_workspace_cannot_grant_access -- --ignored --test-threads=1
cargo test -p plugin-runtime --test scoped_instances failed_update_restores_durable_writes_and_trust_revocation_skips_guest_checkpoint -- --ignored --test-threads=1
# 原生界面的最终显式结果另行记录，不把 ignored 算作通过。
cargo fmt --check
cargo test --workspace --exclude editor-app
cargo check --workspace
cargo test -p editor-app -- --test-threads=1
cargo test -p editor-app real_layout_ -- --ignored --test-threads=1
cargo test -p editor-app real_plugin_tools_preserve_context_shared_intent_and_separate_overflow -- --ignored --test-threads=1
cargo test -p editor-app extensions::markdown_tests::migration:: -- --ignored --test-threads=1 --nocapture
cargo test -p editor-app extensions::ui_package_tests:: -- --ignored --test-threads=1
cargo test -p editor-app code_tracks_provider -- --ignored --test-threads=1
```

原生批次使用上述过滤词直接执行本次 Cargo 编译出的测试二进制，保持串行；日志分别为
`final-native-layout*`、`final-native-tools`、`final-native-migration`、`final-native-packages` 与
`final-native-language`。连续输入的实测为 500,008 字节、373.50 毫秒；光标／焦点保持，旧树只读，
访客完成后来源与当前文档版本相同，见 `final-native-migration.log`。

布局组合最初发现两处历史测试夹具问题：不同身份的 Image 重打包只改 JSON 清单，漏改新增的
TOML 资源清单；SVG 新布局已携带精确 FileContext，旧断言仍要求 file 字段为空。
前者使用现有 TOML 依赖同步两份声明并继续走公共包校验，后者改为精确文件／文本版本及旧图片
区域不可见的行为断言，没有放宽宿主授权或删除场景。仅重跑受影响布局组合；完整基线后的变化
限于 `cfg(test)` 夹具和已有版本的 dev dependency，非 UI 基线证据继续有效。

最终候选 `dd10c46` 的双轴审查共同发现一项 P1：旧安装注册表的 `toolbar:null`／
`toolbar_icon:null` 或 `view_modes` 会在有限迁移前被严格公开类型拒绝，阻断 Manager 启动。
`final-registry-ui-red.log` 通过公开启动入口真实复现；修复仅在磁盘元数据边界去除三个已知字段，
保存不可覆盖的 `registry.before-ui-contract.json` 原始备份，并持久化不可执行标记。
未知当前字段继续报错，新 ZIP 继续严格拒绝旧字段；授权、全局／工作区启用选择、设置和私有数据保留。
显式启用、设置更新或重启不能解除标记，只有通过现行 Package 校验的安装成功后才能恢复实例。

事务注册表和历史 `record.json` 共用该导入边界；先完成旧协议／ID 迁移的原始备份，再写入规范化
注册表，避免两次迁移覆盖证据。真实 SDK 安装／重开为 56.15 秒，历史目录导入为 93.24 秒；
前者保留私有文件及用户设置，后者保留两份工作区数据、快照和恢复备份。
记录见 `final-registry-ui-{red,green,sdk,legacy}.log`。补充基础检查为 `final-reviewed-*`，
非 UI 128 项与完整应用串行 249 项通过；宿主已用最终运行时重建。

`7f0362b` 的窄复核发现设置入口也能准备已禁用组件，因此仅拦截启用仍不充分。
公开 `update_setting` 回归先红；现在在改变设置候选及读取组件前检查安装兼容性，
旧记录的原设置与私有数据保持原样。常规目标 4 项通过；真实 SDK 包设置拒绝、合法安装与
重开恢复用例再次通过，55.71 秒。记录为 `final-registry-setting-{red,green,sdk}.log`。
两个审查轴已核对其余实例创建／恢复接缝，未发现其他同类遗漏；最终修复候选仍需固定提交复核。

## 实际限制

GIF／WebP 使用静态帧；本批不添加动画播放、图片编辑或编码转换。
图片资源最多 8 MiB，文本预览传输最多 1 MiB，预览节点等继续受公开配额限制；
超限显示原因并保留文件或原生编辑入口，不承诺任意大文件完整预览。
Markdown 仍派生只读快照，本批没有复制原工作区未提交的增量解析改动。
原生证据为 GPUI 测试窗口的实际输入、布局与绘制；未声称手工截图或未运行用例通过。

## Standards

等待最终固定提交双轴审查。

## Spec

等待最终固定提交双轴审查。父议题 #61 保持原状，工单 #66 只在最终验收、推送及读回后关闭。
