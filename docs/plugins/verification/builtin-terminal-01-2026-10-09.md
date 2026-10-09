# 内置终端第一阶段验收（#88）

范围：N01、N02、N22–N25，普通 Shell 与内置逻辑快照。旧插件导入和旧发行入口退役归 04；运行、构建和调试归后续工单。基线为 `48e33ba1f6e8232349206b171256b66fac1fd901`，代码审查候选为 `39e6ccb0c5e39ba8bc5d4488eef78eaeb502138b`。候选仅保存固定文件树，不移动当前分支；正式提交在检查完成后进行。

## 实现及证据

- 依赖锁定上游 `alacritty_terminal = 0.26.0`，未复制上游源码、未使用旧 WASM 解析器。原生模块直接投影 Alacritty 网格到本地 Canvas，复用现有受控 PTY、进程树与尺寸合并机制。
- 无终端包的隔离 GPUI 窗口通过底栏第二个图标打开真实 PowerShell，新增同名 Tab 可分别输入；真实选择、Ctrl+C、空白区域选择限制、右键清空、信任撤销经过实际绘制和进程事件核对。
- 保存并重开两个真实 Shell，快捷键 Ctrl+Shift+T/W 和关闭当前 Tab 后剩余恢复 Tab 可输入。恢复只保存逻辑状态，启动新 Shell；初始 ConPTY 查询从保存光标应答，不插入恢复横幅或额外换行。恢复、长输出、宽高调整由原生夹具验证。
- 真实 Neovim 进入 alternate screen，输入中文与 emoji，连续改变宽高，保存实际文件并返回 Shell。双宽字符右半格选择、清空缓冲区保留输入和鼠标模式另有最小回归。
- 复用 SideTabBar 的名称编辑、Enter/失焦保存、左右调整、排序与完整边框用例；复用共享滚动条绘制/拖动与 Canvas IME marked-text/主题用例。终端设置 1 秒空闲隐藏，仅存在滚动历史时提供范围；保持上游拖动捕获行为。
- 原生窗口验收使用独立临时配置及空插件目录，观察到供图图标、标题工具、内边距、竖线光标、蓝色选中 Tab 文字/内边框、正常 PowerShell 提示符、新建同名 Tab 和初始无滚动条。其余输入和主题组合采用 GPUI 原生输入自动化；未将自动化输入称为实体键盘或 Windows 输入法候选窗口验收。macOS/Linux 未在本次 Windows 环境运行。
- 损坏快照包含合法首 Tab 和非法后 Tab：拒绝整个恢复、显示错误，经过自动保存周期后原文件字节不变。保存容量上限只裁剪最旧历史，不丢可见画面和配置。
- 饱和命令/输出队列仍可撤销权限及收到清理完成回执；快速撤销后重新授权不会误关新代进程。两个真实进程回归都先观察修复前失败，再观察修复后通过。

## 检查

环境：Windows、Rust 1.95，现有 GPUI 依赖保持不变；应用测试设置 `RUST_MIN_STACK=16777216`。完整非 UI workspace 测试里的其他实际包 ignored 用例未执行，不计为通过。

| 命令 | 结果 |
| --- | --- |
| `cargo test -p plugin-runtime --lib native_processes::tests -- --test-threads=1` | 2 通过；真实进程撤销与重新授权 |
| `cargo test -p editor-app terminal:: -- --test-threads=1` | 9 通过、1 ignored |
| `cargo test -p editor-app terminal::tests::fullscreen -- --ignored --test-threads=1` | 显式真实 Neovim 1 通过；终端逻辑与夹具与审查前候选相同 |
| `cargo test -p editor-app side_tabs -- --test-threads=1` | 5 通过 |
| `cargo test -p editor-app scrollbar -- --test-threads=1` | 1 通过 |
| `cargo test -p editor-app canvas_ime_preserves_marked_text -- --test-threads=1` | 1 通过 |
| `cargo fmt --check` | 通过 |
| `cargo test --workspace --exclude editor-app` | 所有非 ignored 用例通过，包括 runtime 74 个单元测试 |
| `cargo check --workspace` | 通过 |
| `cargo build -p editor-app` | 通过；原生调试构建 |

前三项最终回归及 workspace 门禁对应候选 `39e6ccb0`；共享控件和 Neovim 证据来自前候选 `6c62203f`，之后仅修改 supervisor 过期代次清理与新增权限测试，未改变绘制、输入、恢复、工具或 TUI 夹具。仍有仓库既有 unused/private-interface 与链接器警告，未扩大范围修复。

## Standards

使用 [code-review](C:/Users/Tmiracle/.agents/skills/code-review/SKILL.md) 的规范轴并行只读审查。最初发现队列饱和可丢权限撤销、错误消息没有本地化；均修复并补回归。最终候选复审：硬性问题 0，判断项 0；代次过滤注释说明了竞态与资源归属。

## Spec

规格轴最初发现缺少 Tab 快捷键、关闭恢复 Tab 后未启动剩余 Shell、清空重建了模式、宽字符右半格选择不正确；均在真实原生夹具或最小行为测试中确认 RED/GREEN。后续发现旧撤销误杀重新授权进程，同样补真实子进程回归。最终复审无遗留发现，无新增范围。

两个轴最终发现总数均为 0。本阶段不宣称整个旧用户切换或整批任务完成。
