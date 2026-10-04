# 08 — 链接导航验证记录

状态：通过。基线 `24a27dc8bbfc2cb16557fc7d28f22bbafa2872ed`；开始时读回 #34 open / ready-for-agent、前置 #28 closed / completed。Markdown `0.8.0`，独立 SDK 示例 `0.15.6`；协议 7、UI 1 与能力版本分别管理。

## 交付行为与边界

实际 ZIP → 原生富文本／图片链接与逐链接键盘焦点 → `ui.links 1.0` 事件 → `editor.navigation 1.0` 版本化请求 → 既有文档打开、最近滚动区的实测块定位或 GPUI 浏览器边界。宿主没有 Markdown ID、语言或扩展名专属分支；网页需 `navigation.external`，相对文档需现有工作区读取权限。

插件按真实 CommonMark 事件生成标题 slug，中文保留、重复消歧；匹配当前节点精确 writer href 后使用原解析 URI。Email 自动链接补隐含 mailto，即使以 .md 结尾也不误作文件；显式含 @ 的普通文件链接仍可打开。片段和文件路径分别严格解码一次，规范化及 junction 解析后检查工作区边界。目标片段等待真实 Opened 回执和目标预览。

效果前核对文档身份、revision、UI revision、活动节点、实例和取消状态。拖动、孤立 MouseUp、旧场景按下后新场景释放、越界路径及未知协议不能导航，不启动 Shell。导航不改文本、选择或 Undo。中英文拒绝原因在工具栏及预览顶部只读栏可见，覆盖仅预览、长文和配额 fallback；源变化后清理旧错误。

## RED 与修正

- 0.7 实际包相对文件点击 RED 19.37 秒 → 0.8 GREEN 20.09 秒。重复中文标题与网页 RED 19.93／19.73 秒；修正 writer 编码中文 href 和目标片段时序。
- 真实图片外链 RED 19.87 秒、逐链接键盘 RED 21.57 秒。补有界 Node.links 与 Base 焦点；长文焦点 RED 的 cue top=2512.5px，pane 62–867px，按最近 Scroll 实测定位后 GREEN 21.60 秒。夹具按上游激活前景窗口，发送完整 KeyDown／KeyUp。
- 通用原生手势 RED／GREEN 覆盖场景替换和拖出释放后重用；窗口 MouseUp 派发结束撤销按下归属。
- 仅预览反馈的确认 RED 20.79 秒：真实点击产生 toolbar 中的锚点错误，但隐藏源码区后 header=None。恢复外置提示后的最终 8 项原生测试通过。此前单字 caption 未触发 guest 错误的失败不计产品 RED；撤销必需权限后 enable 失败为夹具错误，改为公开安装更小声明的实际 ZIP。
- 邮件自动链接 .md 真实访客 RED 返回 Relative；按 LinkType::Email 修正后完整 53 项通过。无效 CommonMark 嵌套图片外链夹具修正有效正向语法，同时保留原语法的实际事件和零激活负向检查。
- 初始英文却断言中文的三项安全失败为夹具语言假设；显式公开 Theme 通知后 5 项全通过，不将失败当成功。

## 最终命令与证据

Windows 隔离目录；Cargo 串行、CARGO_BUILD_JOBS=1；原生另设 RUST_MIN_STACK=33554432。下列日志均在仓库 target 中。

| 命令／入口 | 实际结果 | 日志 |
| --- | --- | --- |
| `editor-app.exe --plugin-cargo plugins/markdown/Cargo.toml test` | 53 passed / 0 ignored，0.01 秒 | 08-final-guest-tests.log |
| `build-plugins.ps1 -Packages markdown -HostExe ./target/debug/editor-app.exe` | SDK 构建实际 ZIP，9.90 秒 | 08-final-markdown-package.log |
| `cargo test -p editor-app extensions::markdown_tests::link_navigation -- --ignored --test-threads=1` | 8 passed / 0 ignored，173.59 秒 | 08-final-native-navigation.log |
| `cargo test -p editor-app extensions::markdown_tests::navigation_safety -- --ignored --test-threads=1` | 5 passed / 0 ignored，150.17 秒 | 08-final-navigation-safety.log |
| `cargo test -p editor-app ui::plugin -- --test-threads=1` | 33 passed / 0 ignored，1.95 秒 | 08-final-native-ui.log |
| `cargo test -p plugin-runtime --test editor_navigation -- --ignored --test-threads=1` | 3 passed / 0 ignored，193.82 秒 | 08-final-runtime-navigation.log |
| `cargo test -p plugin-protocol` | 25 passed / 0 ignored | 08-final-protocol.log |
| `cargo build -p editor-app` → `verify-plugin-sdk.ps1` | 宿主 34.72 秒；仓库外导出、损坏修复和构建 52.27 秒 | 08-final-host-build.log、08-final-sdk-verification.log |
| `cargo test -p plugin-runtime --test sdk_distribution -- --ignored --test-threads=1` | 1 passed / 0 ignored，17.32 秒 | 08-final-sdk-runtime.log |

最终 SDK `4177ca32491745b794a51524b001c15f119ba8e9a30974fac3891ae4f81d9688`，早期摘要未冒称最终验证。最终 ZIP SHA-256 `76c7feb855eff289bda4529d474532490e1dbee4a1c54eb502f02ab6bb912dd3`。反馈 RED 实验后恢复同一已通过 ZIP 与最终源码，不保留实验删除。误用 editor-app SDK 过滤得到 0 tests，不计通过，随后在正确 runtime 测试显式运行。

原生覆盖标题、相对文档／中文片段、图片外链、多 href Tab／Enter／Space、长文焦点、只点击时打开网页、双语仅预览失败、编辑／切页／关闭／停用撤销、单次解码及真实 Windows junction 拒绝。独立非 Markdown 访客沿同一公开请求打开 .txt，验证缺权限、过期请求与合法浏览器入口。实际 runtime ZIP 验证 RichText／Image／Text 能力门禁、惰性和元数据。共享路径规则另跑 ui_images_local：2 passed / 0 ignored，34.30 秒。

## 门禁、审查与限制

`cargo fmt --check` 与访客 `cargo fmt --manifest-path plugins/markdown/Cargo.toml --check` 通过（08-final-fmt.log）。`cargo test --workspace --exclude editor-app` 实际 72 passed / 96 ignored、36 个结果组（08-final-workspace-test.log）；96 个忽略用例没有计为通过。本单涉及的实际 WASM 已按上表显式运行。`cargo check --workspace` 通过，3.11 秒（08-final-workspace-check.log）。文档路径及 `git diff --check` 通过。

独立 Standards：硬性 0、Fowler 判断性 0；独立 Spec：0 findings。按授权“测试与审查 → 提交”，固定基线至工作树及新增文件审查；此时 git log base..HEAD 为空，不伪称已提交差异。审查发现的图片外链、逐链接键盘焦点、长文可见性和仅预览提示均已修复。

浏览器在既有 GPUI 测试边界记录，不依赖真实外网；Windows 原生自动交互为本单平台。保留环境既有 LNK4217、private-interface、未使用 API 警告。代码块高亮、同步滚动和发行继续由 09–11 交付。#26 不修改；普通提交、推送与 #34 关闭读回完成后追加。
