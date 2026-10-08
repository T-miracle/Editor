# GitHub 预览配色、六级标题、工具栏显隐与小幅滚动

日期：2026-10-05。状态：六项修改已实现，定向原生回归、仓库门禁、SDK 与正式产物核对通过；交付 Markdown 0.13.0 与配套宿主。

## 范围与依据

本记录对应用户本轮六项要求，覆盖并替代记录 14 中“十三个格式按钮”和“底栏开关切换整个预览”的当前行为；记录 14 的历史结果保持原样。本轮不提交、推送或变更历史议题。

- 预览采用 GitHub Primer 文档配色；行内代码明确设置正文前景，解决浅色表格中的白色文字。颜色语义参考 [Primer Color](https://primer.style/product/primitives/color/)。
- 深浅 token 读回官方 `@primer/primitives` 发布的 [dark.css](https://cdn.jsdelivr.net/npm/@primer/primitives/dist/css/functional/themes/dark.css)／[light.css](https://cdn.jsdelivr.net/npm/@primer/primitives/dist/css/functional/themes/light.css)：代码块和表头采用 muted 背景，行内代码独立采用 neutral-muted 半透明底色，正文与淡化文字分开。
- 格式按钮默认透明，保留 hover、active、禁用和键盘焦点。十八个按钮包含几何 SVG 的 H1～H6，统一 24 × 24 / 2 px 描边。
- 标题切换光标行或选中行；替换现有级别、同级取消，不拆分行。空行保留本地化占位模板；UTF-8 内容、CRLF、缩进和单次撤销保持。
- 底栏 title 为“隐藏/显示Markdown工具栏”／“Show/hide Markdown toolbar”；只切换源码顶部工具栏，预览和同步控制保持可用。选择按工作区保存，切换普通文件不退回面板开关。
- 小幅预览滚动保留实际源码亚像素位移，延迟同步不得撤销手动位置。

## 失败复现与定位

通过实际 ZIP → 公共 Manager → 原生输入验证，未修改访客私有滚动状态。

1. 行内代码传给 Base 的 `HighlightStyle.color` 为 None，未明确覆盖上游 accent 前景。`document_rich_text_preserves_role_colors_in_both_themes` 先失败；修正为显式正文前景。
2. 光标位于“中文标题”中间时，旧标题按钮产生 `中\n# heading\n文标题`。`heading_switches_current_line` 先失败；标题现在一次替换目标整行，再保持逻辑内容光标列。
3. 旧包底栏按钮点击后预览消失。`status_icon_follows_sync` 先失败；新版插件通过公开 `Document.editor_toolbar_toggle` 声明工具栏专属行为。
4. 普通段落的 1/2/5 px 测试不能复现小幅问题；小于实际布局像素的 0.1 px 输入也未实际移动预览，不作为缺陷证据。大图高 1000 px 的真实预览滚动 1 px 后，源码仍为 0 px，形成有效 RED。
5. 根因是源定位在**首次精确提议之前**使用 0.5 px 容差，吞掉映射到源码的小幅位移。搜索阶段保留原容差；精确定位采用 0.001 px 进度阈值，执行一次提议后才允许原有布局舍入误差，避免重复逼近造成回流。
6. 补充底栏键盘回归发现，Space 已改变显隐状态，但源码 Dock 缓存未刷新。开关显式通知编辑面板，底栏使用面板持有的稳定焦点句柄，避免依赖鼠标造成的焦点变化刷新。

## 公开边界与交付

- `editor.toolbar` 提升至 1.1.0；可选 `Document.editor_toolbar_toggle` 为本地化 caption，最多 4096 字节。运行时校验实例、面板所有权、读取权限及能力版本，不按插件身份分支。
- UI 文档版本仍为 1，协议仍为 7；其他插件省略声明，继续使用原有面板开关。
- 任意插件可用 `github` / `github-muted` 和 `toolbar_button` 主题 role；现有用户主题覆盖优先。
- Markdown 清单、贡献、Cargo 和锁文件版本统一为 0.13.0；要求 `editor.toolbar ^1.1`，独立构建仍经宿主内嵌 SDK。

## 实际验证

已执行的定向 Windows 原生用例共 22 项，均显式使用 `--ignored --nocapture`，0 ignored；没有重跑历史 62 项全量包验收。所有用例安装交付的真实 ZIP，不绕过 Manager、DocumentSession 或原生输入。

| 过滤词 | 通过数 | 覆盖 |
| --- | ---: | --- |
| `icon_toolbar` | 2 | 十八图标、深浅主题、窄窗换行、底栏位置与 toolbar-only 显隐、重复 Space、普通文件切换 |
| `format_toolbar` | 4 | H1～H6 当前行切换与同级取消、撤销、已有格式操作和键盘 |
| `synchronized_scroll` | 6 | 双向、长文、语法结构、文档切换、取消及钳位 |
| `first_open` | 1 | 首次打开后不被同 revision 刷新拉回顶部 |
| `source_layout` | 2 | 实际源码高度及输入、缩放 |
| `modes` | 2 | Markdown 与 SVG 三种视图 |
| `link_navigation::synchronized` | 2 | 链接跳转与滚动事务 |
| `small_scroll` | 2 | 大图片和长表格 1/2/5 px 真实手动滚动、亚像素源码位移、延迟不回弹 |
| `fresh_markdown_first_use` | 1 | 新安装确认、启用高亮与 UI、禁用偏好保留 |

记录：`target/markdown-adjust-native-result.json`、`target/markdown-adjust-extra-small_scroll.log`、`target/markdown-adjust-footer-final.log`、`target/markdown-adjust-fresh.log`。早期键盘失败日志保留用于定位，最后复测为 2/2 通过。

插件独立单元测试：经 `target/debug/editor-app.exe --plugin-cargo plugins/markdown/Cargo.toml test --lib`，65/65、0 ignored；见 `target/markdown-adjust-guest.log`。H1～H6 在实际 14 px 图标尺寸及深浅底色下检查，图见 `target/markdown-heading-icons.png`。

仓库门禁全部 exit 0：`cargo fmt --check`；`cargo test --workspace --exclude editor-app -- --test-threads=1` 为 117 passed / 116 ignored；`cargo check --workspace`；`cargo test -p editor-app -- --test-threads=1` 为 247 passed / 106 ignored。默认 ignored 计数不作为通过数；上表的包用例另行显式执行。最新深浅 GitHub role 及透明按钮、行内代码前景测试均在编辑器默认测试中通过。完整命令、耗时和摘要见 `target/markdown-adjust-gates-result.json`。

正式宿主经 `cargo build -p editor-app --release` 构建；`target/release/editor-app.exe` 与 `dist/editor/editor-app.exe` SHA-256 一致：`49fc0704ecc44d72a58d973081a778e4594cc0406b7ec33a36e34f827ecd2aa1`，见 `target/markdown-adjust-release-result.json`。

插件经 `./scripts/build-plugins.ps1 -Packages markdown -HostExe target/debug/editor-app.exe -Output dist/editor/plugins` 独立构建为 0.13.0。正式 `dist/editor/plugins/markdown.zip` 与测试入口 `dist/plugins/markdown.zip` 完全一致；34 条目、22 个 SVG、2 份 WASM grammar，SHA-256 为 `82b251fbab1e08e3e0785904fa8452119f65fb8b85c292a667a210b73c7bf783`，两处 bundle catalog 同步匹配此 hash；见 `target/markdown-adjust-package-result.json`。ZIP 保留已测试的访客产物，随后宿主 SDK 的文档更新不改变其公开 Rust 契约。

SDK：`./scripts/verify-plugin-sdk.ps1 -HostExe target/release/editor-app.exe` 通过导出、缓存修复及仓库外 WASM 构建，见 `target/markdown-adjust-sdk-final.log`。使用其真实独立包显式执行 `cargo test -p plugin-runtime --test editor_edit toolbar_visibility_toggle -- --ignored --nocapture`，1/1、0 ignored，见 `target/markdown-adjust-contract.log`；`=1.0.0` 在包检查阶段被明确拒绝，`^1.1` 正常安装并公开 caption。初次断言错误地预计在安装阶段拒绝，已修正测试，未为此改变运行时行为。

文档链接和 `git diff --check` 通过。现有 Wasmtime/Tree-sitter Windows 链接警告及未用代码警告不影响本轮编译和测试，未扩大范围修正。

原生测试使用 Windows GPUI TestAppContext 的实际布局、输入与公开包管线；没有控制用户正在运行的编辑器窗口。其他操作系统未验收。必须同时使用新版宿主和更新已安装的 0.13.0 插件，重新打包目录不会自动升级用户已安装版本。
