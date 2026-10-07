# 快捷键面板 C 原型修正验收

日期：2026-10-07。状态：本轮 UI 修正与行为验收完成。

依据：[当前规格](../specs/keyboard-shortcuts.md)、用户选定的 [C 原型](../specs/assets/keyboard-shortcuts-c.png)，以及本轮“遵循原型、增加恢复默认和删除绑定、修复顶部 tab 圆角”的补充要求。三张工单的历史交付状态见[原阶段记录](keyboard-shortcuts.md)；本轮没有重新关闭工单或改写父议题 #77。

## 实现与验证对象

- 在 `C:/Projects/RustProjects/Editor-shortcuts-main` 的 `main` 实施，固定比较点为 `f22d45235aec966fc100f35370cfab30db59f4db`；审查命令为 `git diff f22d452 -- crates/editor-app docs/ui website`，提交前的差异包含未提交修正。
- 弹层按 C 比例改为约 760 × 680 逻辑像素，小窗口仍缩小；两个外侧 tab 显式绘制顶部圆角，保留活动下划线及键盘焦点提示。
- 搜索区增加放大镜和独立键盘图标按钮；键帽列起点对齐，未绑定按钮保持紧凑，次要管理图标在悬停或键盘焦点时显示。键盘与恢复 SVG 通过现有额外图标资源包嵌入。
- 编辑行左侧为描述及录入说明，右侧为录入框，下方依次为恢复默认、删除绑定、取消、保存。普通列表不再重复显示删除按钮，展开时不重复显示旧键。
- 删除针对选中的原已保存绑定，保留其他组；新增草稿禁用删除。恢复默认继续使用冲突预览，取消确认不写配置。恢复确认期间另一窗口改变同操作时，原列表校验阻止旧确认覆盖新绑定。
- 长序列在列表与录入框内换行并撑高行；按键搜索保持固定高度的单行水平视口，避免侵入列表。

主工作区已有 `Cargo.lock` 的 `patch.unused` 记录及未跟踪的 `plugins/configuration-example/Cargo.lock`，均保留且不提交。前者使直接 `--locked` 检查拒绝更新锁文件，因此建立临时校验工作树 `C:/Projects/RustProjects/Editor-shortcut-ui-check`，基线同上、使用该提交的锁文件。10 份修改源文件逐一核对：统一 CRLF/LF 后文本完全一致；其中两份文件仅换行编码不同。校验目录不承担实施或发布，构建共用 `CARGO_TARGET_DIR=C:/Projects/RustProjects/Editor/target`。

## 自动验证

以下最终 Rust 命令在校验工作树执行，真实结果不包含 ignored 为通过。

| 命令 | 结果 | 日志（主工作区根） |
| --- | --- | --- |
| `cargo test -p editor-app shortcuts --locked -- --test-threads=1 --nocapture` | 20 passed、0 failed、5 ignored、507 filtered out | `shortcut-ui-targeted-final.log` |
| `cargo fmt --check` | 退出码 0 | `shortcut-ui-fmt.log` |
| `cargo test --workspace --exclude editor-app --locked` | 196 passed、0 failed、167 ignored；50 个结果块 | `shortcut-ui-workspace-final.log` |
| `cargo check --workspace --locked` | 退出码 0 | `shortcut-ui-workspace-check.log` |
| `cargo test -p editor-app --locked -- --test-threads=1` | 384 passed、0 failed、148 ignored | `shortcut-ui-app-full-final.log` |
| `cargo build -p editor-app --locked` | 退出码 0；用于本轮原生验收 | `shortcut-ui-build-final.log` |
| `cargo test -p editor-app shortcuts_real_plugin_lifecycle_restoration_conflicts_require_decision --locked -- --ignored --test-threads=1 --nocapture` | 1 passed、0 failed、0 ignored、531 filtered out | `shortcut-ui-real-restore-final.log` |
| `npm test`（主工作区 `website/`） | 15 passed、0 failed、2 skipped（未构建搜索索引） | `shortcut-ui-website.log` |

复用既有粗粒度应用场景，没有新增测试名称或为四个按钮各建一套入口。覆盖右侧录入位置、580 × 430 窗口的四按钮边界、删除时保留另一绑定、新增禁用删除、恢复冲突取消/替换、两真实编辑窗口的并发修改，以及长两步序列撑高与 500 × 430 按键搜索上下边界。正常控件、文档修改、跨窗口分发和持久配置仍经真实应用入口验证。

真实 WASM 夹具沿用已通过公开宿主 SDK 构建的 `capability-example.zip`，SHA-256 为 `F7B36538954C71CD430019F434901B3B57E86C6A1DE9D3A8B86807DDEB8FE2C1`，复制到校验目录后散列一致。本轮中间版本已显式执行五项真实快捷键场景，5 passed、0 failed（`shortcut-ui-real-plugin.log`）；后续并发恢复保护改变预览路径，最终版本再次显式执行受影响的恢复冲突场景。没有把中间版本的五项结果或其他未执行的 ignored 当作最终全量通过。

迭代中实际遇到并修正/绕开的事项：

- 主工作区 `--locked` 因已有锁文件附加记录失败，改用上述隔离校验目录，未删除附加记录。
- 新布局断言首次编译缺少 `px` 导入（`shortcut-ui-wrap.log`），补齐后针对性及最终全量测试通过。
- 非 UI 测试首次链接失败 `LNK1104: msvcrt.lib`（`shortcut-ui-workspace.log`）；已安装库确实存在，当前终端的 `LIB` 为空。仅为后续校验进程加载已安装 VS 18 的 `VsDevCmd.bat -arch=x64 -host_arch=x64`，随后成功；没有安装或升级工具链。
- 搜索单行修正前的全量应用测试也为 384 passed（`shortcut-ui-app-full.log`），因审查后的搜索布局变更，最终全量另外重跑；前者不冒充最终记录。

构建仍有既有 Rust 与 Wasmtime/Tree-sitter 链接警告。未修改 SDK 契约，不重复 SDK 分发矩阵；其他真实包 ignored 没有全量执行。

## Windows 原生验收

使用最终构建的 debug 程序、`target/shortcut-native-ui` 专用工作区。现有 `ME_EDITOR_PROFILE_HOME` 隔离快捷键配置，`ME_EDITOR_PLUGIN_HOME` 指向专用空插件目录；其余会话仍按应用原有规则保存。只操作本任务进程，验收后关闭专用进程。

- 浅色和深色都观察到两外侧顶部圆角、固定搜索/底部、对齐的键帽列、键盘及恢复图标。
- 实际点击 Shift+Enter 绑定，展开右录入框和四个按钮。点击删除后该行显示“未绑定”，专用 JSON 中对应操作为 `[]`；重新进入新增态，删除按钮显示禁用。
- 点击行内恢复默认后 Shift+Enter 键帽恢复，JSON 的 overrides 清空；没有通过手写配置冒充按钮效果。
- 深色在设置页实际选择后检查。保存文档行实际录入 Ctrl+Alt+U，蓝色边框内显示键帽，保存按钮可用；点击保存后列表显示新键，JSON 持久记录 `ctrl-alt-u`。原用户快捷键配置未因此改变。
- 原生截图验证圆角与外观；长两步、小窗口、缩放、冲突和并发边界由本轮 GPUI 应用场景覆盖。本轮未声称重演历史中文 IME、所有插件组合或原生两步时限。

实际截图：[浅色浏览](assets/keyboard-shortcuts-correction-light.jpg)、[深色行内四按钮及录入](assets/keyboard-shortcuts-correction-dark.jpg)。原型图仍是设计依据，二者不混作同类证据。

## Standards

只读并行规范审查：P0/P1/P2 为 0。悬停按钮保留 Base 焦点与激活；删除目标、恢复原列表校验、长行高度、注释、双语资源和本地 UI 接口符合约定，未留下需处理的坏味道。审查未运行 Cargo，并排除已有两份锁文件改动。

## Spec

只读并行规格复审：P0/P1/P2 为 0。此前搜索键帽在固定高度内换行的 P2 已通过单行水平视口修复；四按钮、删除保留其他组、恢复冲突及并发保护、顶部圆角和键盘焦点符合用户补充要求，未发现范围扩张。

两轴各 0 项未解决发现；最终自动验证与原生验收由主执行任务完成。

## 证据身份

| 日志或截图 | SHA-256 |
| --- | --- |
| `shortcut-ui-targeted-final.log` | `FF20F79346FEDD33B90778B9BD21BD2AB3FE10FCA9C5F65D9E278361FBAE043A` |
| `shortcut-ui-build-final.log` | `C43E07636AE4CD4BB46E3D78A941E451E83F677DC19057F107945951477F81E2` |
| `shortcut-ui-fmt.log` | `E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855` |
| `shortcut-ui-workspace-final.log` | `7705B2C2711E2DB7DC3DE7EE3D5A59B35DE9F498937BD11F8E4FB83F489506EA` |
| `shortcut-ui-workspace-check.log` | `A7C446CA6F219E0E330DBAD1E6100290F848C9E73EF86FDC16DE8D02A694DB64` |
| `shortcut-ui-app-full-final.log` | `8F0C7F2616386001653DF269E7049A292D6DAE246395E22260BDF3B15026AE49` |
| `shortcut-ui-real-restore-final.log` | `8D851EC21FB7B951176F10245146188EF3644769D21D3B80B93D59920F452C8E` |
| `shortcut-ui-real-plugin.log` | `4D7021D5D840A2D03F9E6291E5981462F15936BA420C8BAB45A661F4021384D1` |
| `shortcut-ui-website.log` | `F790220A220E0EB3086500310DE575E3A2D06A14339237447598D714D6302B26` |
| `assets/keyboard-shortcuts-correction-light.jpg` | `32F41568E83ABBE2C2CE2918CBF05432A9165DA4210CDA5862BB70CF6ABF1BE2` |
| `assets/keyboard-shortcuts-correction-dark.jpg` | `C83ADFE85DDB9213F6342693DBFAA5101547E92DF22D7D532D80FED03787D684` |

返回[阶段验收记录](keyboard-shortcuts.md)与[原生 UI 目录](../README.md)。
