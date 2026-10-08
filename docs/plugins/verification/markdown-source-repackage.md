# Markdown 源码迁入与重新打包

日期：2026-10-04。

状态：源码迁入、插件测试和重新打包完成；主项目宿主能力集成及本次原生界面验收未执行。

后续更新：用户随后要求“全部补齐回主项目”，宿主能力已合回主目录 `main`。新版宿主和包的验证单独记录在[主项目集成验收](../../../plugins/markdown/docs/verification/12-main-project-integration.md)；下文保留仅迁入源码时的实际结果和限制。

## 变更范围

用户要求将已有完整 Markdown 插件源码放入主项目插件目录并重新打包。来源为 `C:/Users/Tmiracle/.codex/worktrees/markdown-plugin/Editor/plugins/markdown`，复制到主项目 `plugins/markdown`，保留源码、grammar、查询、图标、许可证、清单、说明及历史工单／验收，排除来源的 target 构建缓存。来源未删除或改写。

迁入时将包及 Cargo 版本从 0.11.0 同步提升为 0.11.1；不修改 Markdown 业务代码或权限。修正资料归档链接，声明匹配宿主要求。主 workspace 排除独立访客项目，打包脚本支持显式选择 markdown，发布 ZIP 不包含源码、Cargo 文件或缓存。

已有工作区改动保持原状；本次未提交、推送、安装插件或修改用户插件注册记录。

## 构建与测试

- 匹配宿主：`C:/Users/Tmiracle/.codex/worktrees/markdown-plugin/Editor/dist/editor/editor-app.exe`；其内嵌 SDK 已包含清单要求的新增能力。
- `./scripts/build-plugins.ps1 -Packages markdown -HostExe <上述匹配宿主路径>`：通过，实际重新编译 `markdown-guest 0.11.1` 为 wasm32-wasip2，输出 `dist/plugins/markdown.zip`。目标工具链原已安装，未安装缺失工具。
- `cargo fmt --check`：通过。
- `cargo fmt --manifest-path plugins/markdown/Cargo.toml --check`：通过。最初尝试经 `--plugin-cargo` 执行 fmt 被 CLI 拒绝，该入口仅支持 build、check、test；改用 Cargo 格式入口后通过。
- 匹配宿主 `--plugin-cargo plugins/markdown/Cargo.toml test`：64 passed、0 failed、0 ignored。
- `cargo test --workspace --exclude editor-app`：通过；该命令中的 ignored 集成项未执行，不计为通过。
- `cargo check --workspace`：通过，保留主项目既有 private_interfaces 和未使用代码警告。
- workspace 检查设置 CARGO_TARGET_DIR 为 Markdown 工作树既有 target 以复用缓存，编译源仍是本主项目；未将另一工作树源码冒充主项目检查。
- `git diff --check`：通过；仅有已有 LF／CRLF 提示。
- ZIP 结构检查：清单版本 0.11.1、protocol 7、12 项必需能力；包括主组件、两份 grammar、查询、视图 SVG 与许可证；无源码／缓存泄漏。首次包 SHA-256：`f98e077fee35dc326f2bda95e20cf1a9ce0664121741b16af67f37c455c9374a`。

### Windows PowerShell 5.1 打包兼容修正

用户在 Windows PowerShell 5.1 中运行脚本时，Path.GetRelativePath 因 .NET Framework 未提供该方法而失败。实际以 Windows PowerShell 5.1 对 TOML 资源包执行同一脚本，复现精确错误；修复为移除规范化插件目录前缀并检查目录边界，ZIP 路径仍使用正斜线。

新增 `scripts/verify-plugin-packaging.ps1 -HostExe <已有宿主路径>`，通过真实打包脚本检查资源入口及长度、路径可移植性和无源码泄漏。Windows PowerShell 5.1.26100.9444 与 PowerShell 7.6.5 均通过，分别核对 11 个 TOML 包条目。随后 Windows PowerShell 5.1 实际重新构建 Markdown 包成功；格式、非 UI workspace 测试与 workspace 编译检查再次通过。ignored 项仍不计为已验证。

修正后重新打包的 Markdown 0.11.1 SHA-256：`d7274de9e4584601b1c469499889cd10b7b1be5d53d1c7cc75cd8656b74d9cf5`。仅打包路径实现变化，未修改插件行为或清单；宿主能力限制保持下文所述。

## 兼容性与限制

主项目当前宿主未包含新 Markdown 所需的 editor.toolbar、editor.presentation、editor.viewport、原生富文本等公开扩展；仅复制插件源码不补齐这些宿主接口。本次使用匹配 SDK 完成打包，不表示新包能在主项目旧宿主中激活。

默认打包集合已补入 Markdown，发行脚本调用也会选择它；新增 `verify-plugin-packaging.ps1 -DefaultPackages` 检查省略 Packages 时的八个插件产物。构建仍要求匹配 HostExe；主项目宿主能力尚未集成，不能将包选择修正当作宿主兼容完成。没有移植宿主功能、合并 Markdown 分支或把历史 0.11.0 的原生验收记为本次新验收。

默认集合回归验证：Windows PowerShell 5.1 使用上述匹配宿主执行 `verify-plugin-packaging.ps1 -DefaultPackages -HostExe <匹配宿主路径>`，通过；在新建的独立输出目录实际生成全部八个 ZIP（包括 markdown.zip），同时 TOML 的 11 个资源条目校验通过。`git diff --check` 通过。

源码与包交付均已完成；新版工具栏、分栏、同步滚动和其他 UI 需在匹配宿主中使用，本次未操作用户当前运行程序或执行新的原生交互验收。

关联：[源码说明](../../../plugins/markdown/README.md)、[历史总方案](../../../plugins/markdown/docs/spec.md)、[插件文档入口](../README.md)。
