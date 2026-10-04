# 10 — 双向同步滚动验证记录

状态：验收通过。基线 `783498a9697354ca9a4505048d57a42def491529`。2026-10-04 开始时读回 #36 open / ready-for-agent、直接前置 #29、#31 closed / completed；09 已推送且 #35 closed / completed。本单提交、推送和 tracker 关闭读回另记录于后续交付段，不用尚未生成的 SHA 充当证据。

主接缝为实际 Markdown ZIP → 公开 Manager → 原生源码／预览滚轮 → 两侧真实可见段落。公开 `editor.viewport 1.0` 将源视口通知、版本化定位与预览块事件关联；内容映射由插件的 SourceRange 提供，程序性来源不会反向触发。链条开关按工作区持久化，只在 Split 启用。

## 实现与实际包

`editor.viewport 1.0` 同时要求 `ui.richtext`、`editor.read`、工作区作用域和已有编辑面板归属。源／UI revision、真实 UTF-8 边界、活动 Scroll 及映射块在通知、请求和最终布局三处校验。独立包不声明模式图标也可使用分栏与链条开关，宿主没有 Markdown ID／扩展名分支。

源码读取 Base 的真实可见行、软换行和原生滚动几何，定位最多 48 次布局步；预览读取当前块高度和最近 Scroll 归属，嵌套 Scroll 的框与内容都不能成为外层视口的定位目标。程序定位回显非零 origin 一次，不回授、不重新发布 UI 场景；快输入只保留一个请求和最新意图，A→B→A 会撤回 B。图片、宽度、字体与高度变化使用 layout 标记保留最后手动驱动侧。

键盘监听在焦点祖先的 capture 接缝登记，滚轮和鼠标继续交给 Base。按住至全局释放保留手动归属，期间迟到定位返回 Cancelled，移出分栏也不改变这条规则；同源主题重测保留归属，真正隐藏、切走文档与实例退役清理归属，避免隐藏期间释放后遗留取消状态。

交付包 Markdown `0.10.0`，独立 SDK 示例 `0.15.8`；协议维持 7，新增能力版本独立为 1.0。最终公开 SDK digest：`cc5c5f132b293e67927196f5fa7e12d062108ae96bd9addc0ad63abd76049dc8`。`dist/plugins/markdown.zip` SHA-256：`f2bdabd00371331d397555a870859f7ee17be31f4598d14ba954e82d4085ef05`。

## RED → GREEN 与边界修正

- 旧 0.9 实际 ZIP 的源码滚轮回归 RED：源码已移动而对应预览段落仍在窗外；接入通用视口后正反向可见段落对应，关闭同步后保持独立。
- 公开 UI 文档缺少 source 的绑定测试 RED → GREEN；源码定位算法四项真实布局行为 RED → GREEN；guest A→B→A 请求合并 RED → GREEN。
- 原生键盘 RED → GREEN；按下后迟到定位、移出分栏继续按住、同源主题变化均补行为回归，持有手势期间定位 Cancelled，真实块位置不被覆盖。
- 实际包“按下源码 → 切到普通文件 → 在普通文件释放 → 返回 Markdown”真实 RED（预览块在第 104 行、源码仍在第 0 行）→ GREEN；真正撤销观察器显式结束手势。另验同实例隐藏后释放再显示。
- 独立运行时初批 2 通过、1 夹具失败：不具 editor.read 的 editor 面板在安装声明阶段已被合法拒绝，修正为普通面板后剩余权限／作用域测试通过。未放宽生产校验。
- 测试窗口必须显式推进布局帧；反向定位和小窗口变化按公开 48 步预算等待，未将失败断言改为整篇百分比或放宽段落对应。

## 命令与结果

Windows 原生 GPUI 测试；`CARGO_BUILD_JOBS=1`，UI 测试 `RUST_MIN_STACK=33554432`。Cargo 串行执行，网络长图使用隔离 loopback 边界。以下均为实际执行，ignored 另行显式运行：

| 命令 | 结果 |
| --- | --- |
| `cargo build -p editor-app` | 通过；最后源码观察器撤销修正后再构建 16.75 s |
| `./scripts/verify-plugin-sdk.ps1 -HostExe ./target/debug/editor-app.exe` | 仓库外独立构建、完整 SDK 导出与修复通过，53.43 s |
| `./scripts/build-plugins.ps1 -Packages markdown -HostExe ./target/debug/editor-app.exe` | 实际 WASM ZIP 通过，16.02 s |
| 宿主 `--plugin-cargo plugins/markdown/Cargo.toml test --lib` | 55 通过、0 跳过 |
| 宿主 `--plugin-cargo plugins/capability-example/Cargo.toml check` | 通过，30.36 s |
| `cargo test -p plugin-protocol` | 28 通过、0 跳过 |
| `cargo test -p editor-app ui::plugin -- --test-threads=1` | 43 通过、0 跳过 |
| `cargo test -p editor-app editor::viewport -- --test-threads=1` | 4 通过、0 跳过 |
| `cargo test -p plugin-runtime --test sdk_distribution -- --ignored --test-threads=1` | 实际仓库外包 1 通过，18.52 s |
| `cargo test -p plugin-runtime --test editor_viewport -- --ignored --test-threads=1` 及修正夹具后的过滤重跑 | 独立包 3 个不同验收场景通过：当前绑定、权限作用域、请求形状／归属／退休 |
| `cargo test -p editor-app extensions::markdown_tests::synchronized_scroll -- --ignored --test-threads=1` | 最后实际包 6 通过、0 跳过，154.54 s；包含同实例隐藏后释放再显示 |
| `cargo fmt --check`，guest／example 各自 `cargo fmt --manifest-path … --check` | 通过；最后宿主改动后的 workspace 格式复检通过 |
| `cargo test --workspace --exclude editor-app` | 75 通过、108 ignored，38 组；跳过项不计通过 |
| `cargo check --workspace` | 最后宿主改动后复检通过，1.75 s |

## 原生覆盖与限制

默认分栏双向同步；关闭后独立；程序性回执不产生反馈及 UI revision 循环；文本、选区与磁盘内容不因滚动改变。链条原生 Space／Enter 操作、选中态、仅编辑／仅预览禁用、工作区重开／切文档记忆及另一工作区独立；无模式图标的独立 SDK 包也有链条。迟到源定位在编辑、切换、关闭重开、按住与停用后不可作用到当前文档。延迟长图、GFM 表格、中文软换行、真实分割线拖动和 950 × 700 窗口重排验证真实可见内容块。

本单没有新引入 IME／编辑事务；前序实际输入验收继续有效。原生自动化与仓库测试不冒充外部桌面输入法的人工操作。完整 M01–M16 与发行安装在工单 11 执行。

## Standards

独立审查发现的键盘接缝、持续手势和真正撤销观察器边界均已修正；最终复审硬规则 0、Fowler 判断性发现 0，已独立核对实际日志与资源 hash。

## Spec

独立审查的同源主题重排与 hide／release 边界已补真实 RED → GREEN；最终复审 0 项未解决问题、未发现缺项或范围蔓延，已独立核对实际原生 6/6 与门禁。

## Git 与 tracker 交付

已普通提交并推送 `67331885717aa80a0366d029a56954a16ed5cc2b` 到 `origin/codex/markdown-plugin`，`ls-remote` 核对完整 SHA 相同。随后 PATCH #36，再以独立 GET 读回 `closed / completed`。#32–#36 直接前置均读回 `closed / completed`，#37 `open / ready-for-agent`；未修改父方案 #26。下一前沿为工单 11。
