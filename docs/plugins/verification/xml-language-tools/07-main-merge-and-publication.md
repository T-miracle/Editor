# XML 合并主分支与工作区发布

2026-10-09，用户授权提交并推送本仓库全部工作区的分支改动，并明确要求 XML 分支先推送、再合并主分支。本记录区分各分支发布与 XML 主分支集成，不继承历史验收结果。

## 发布与合并

开始时核对 12 个工作区，其中 7 个存在未提交改动。原始文件、二进制补丁、状态与候选检查日志保存在本机 `$env:USERPROFILE\.codex\workspaces\Nanobug\github-all-worktrees-20261009`。未使用强制推送或历史重写。

远端返回仓库迁移提示后，核对 Nanobug 仓库的 main 与原远端相同，再将 origin 更新为 `https://github.com/T-miracle/Nanobug.git`。站点的既有 Pages 基址不在本轮迁移范围内。

| 工作区分支 | 已核对的远端提交 | 处理结果 |
| --- | --- | --- |
| main | `842552e9bbcf20c556b54ed7c271857d1a2360aa` | 安装分发、增量预览、插件开发和资料分别提交；随后完成 XML 代码合并并推送 |
| codex/docs-site | `0ed129d90a8bcbc50ffe9d62f709a45bb6babee7` | 原已同步，读回远端核对 |
| Editor-run-debug-build | `eb36d3404ff48813c9077f47634ef6657f63faf5` | 保留独立 SDK 构建的锁文件元数据 |
| codex/explorer-file-transfer | `b0b601d81f2f7ebfb63efe4697169a8e24bf1ed3` | 提交配置示例锁文件，并推送此前本地提交 |
| codex/host-messages | `3f03461ca76a00b8db22a8fd2d6e1f0fd6443a98` | 原已同步，读回远端核对 |
| codex/keyboard-shortcuts | `ebe40c270f2ceb616b12eca0125b461648ce4548` | 快进到完整快捷键实现；待提交旧测试已被上游更完整的同名测试覆盖，原内容保留在备份与 stash |
| codex/markdown-plugin | `0e385c4dcabc5f1cb8d75d046a69f9f6db9f8720` | 归档历史资料、修正重复目录与相对链接，保留当前正本 |
| codex/merge-plugin-ui-main | `db53d2d0bcaae7ca3106f83227e8d1ef902215af` | 发布已有分支提交 |
| codex/plugin-development | `a0a396f1049744939f95e8a2be1a060460108e8f` | 与共享主分支内容对齐，独立保留工作区参考图片 |
| codex/plugin-ui-decoupling | `33c5bc1060f877d3e892b9785b388c6b5517a9f7` | 原已同步，读回远端核对 |
| codex/run-config-main-merge | `ae63225afd6e8a362c6f186d7374323e97405469` | 为原 detached 工作区建立分支并发布 |
| codex/xml-language-tools | `87ef94a04d44737202d99c825aa977258e662bc4` | 分别提交 Windows 原子替换恢复与大纲交互改动，先推送后合并 |

XML 代码合并提交为 [842552e](https://github.com/T-miracle/Nanobug/commit/842552e9bbcf20c556b54ed7c271857d1a2360aa)，父提交为 `ab49a27` 与 `87ef94a`；普通推送后读取远端 main，SHA 与本地一致，并确认 XML 提交是 main 的祖先。表中 main 记录代码合并时的提交；本验收文档作为后续文档提交交付。

## 集成处理

- 保留主分支插件项目打包、隔离开发、增量视图与快捷键配置，以及 XML 分支语言编辑、结构、大纲和 Windows 状态替换能力。
- 文档快捷键共享初始化和原生源码作用域。新增格式化、重命名的中英文操作名称与未绑定元数据；原生回归检查重复初始化不覆盖已有绑定，并通过可见快捷键配置查找新操作。
- 纯语言回调在恢复增量视图之前拒绝 `view_patches`，继续执行快照、权限、能力、结果身份及边界校验。
- XML 增加共享 `nanobug-plugin.json`，清单、guest、锁文件及贡献版本同步提升为 `0.2.1`；协议仍为 7，公开 SDK 仍为 `0.2.0`。HTML、JavaScript 采用 XML 分支的 `0.3.0`，共享描述收录原生服务和许可证。能力验收示例为 `0.17.0`。
- 删除 XML 分支带入的旧仓库脚本，当前 main 不保留 `scripts/`。构建直接调用 Cargo 和宿主 CLI；源码重建说明使用直接工具。中英文 README 与双语 SDK 页面同步交付。

## 本轮验证

Windows x86_64，Rust 1.95、已安装 MSVC 与 Windows SDK、Node 24.19.0。链接环境仅在检查进程设置。普通测试的 ignored 数量保留，下列真实包测试另行显式执行。

| 检查 | 实际结果 |
| --- | --- |
| `cargo fmt --check`、`git diff --cached --check` | 通过 |
| `cargo test --workspace --exclude editor-app` | 合计 221 passed、0 failed、181 ignored |
| `cargo check --workspace` | 通过 |
| `cargo test -p editor-app -- --test-threads=1` | 447 passed、0 failed、188 ignored；包含 SDK 导出、插件开发、Windows 恢复、大纲、停靠、快捷键及新增集成回归 |
| `website/`：`npm ci --no-audit --no-fund`、`npm run build` | 构建及搜索索引成功，17 passed、0 failed、0 skipped |
| 宿主 `--plugin-package`：xml、html、javascript、capability-example，`--output dist/plugins --release` | 4 个 ZIP 完整校验成功；检查 README、服务、许可和包根布局 |
| `cargo test -p editor-app native_outline_package_navigation_follow_and_revocation -- --ignored --test-threads=1` | 1 passed、0 failed，42.70 s；实际 XML 和陌生 ID 包的原生大纲导航、跟随与撤销 |
| `cargo test -p editor-app installed_xml_formatting_preserves_text_and_honors_project_options -- --ignored --test-threads=1` | 1 passed、0 failed，251.34 s；实际 LemMinX、项目选项、保存及原生 Undo/Redo |
| `cargo test -p plugin-runtime --test xml_structure -- --ignored --test-threads=1` | 2 passed、0 failed，82.11 s；独立结构、精确范围及显式 2 MiB 栈上的深层源码边界 |
| `cargo test -p plugin-runtime --test pure_language_completions -- --ignored --test-threads=1` | 1 passed、0 failed，85.91 s；实际 SDK WASM 的权限、拒绝 IO、来源身份及退役 |

真实测试的兼容文件名 `dist/plugins/xml.zip` 等通过直接复制本次版本化 ZIP 准备；能力夹具同样直接复制到 `target/plugin-api-test/capability-example.zip`。已有夹具先备份，未调用旧脚本。

本轮初始失败保留在日志：MSVC 自动探测的链接环境找不到 `msvcrt.lib`，显式选择已安装的 VC 环境后非 UI 测试与编译均通过；新增回归最初访问私有模块，修正为测试已有公开入口；随后可见操作断言未选择全局标签，修正夹具交互后完整应用测试通过。没有删除断言、跳过失败测试或扩大内部可见性。

## ZIP 身份与限制

| 产物 | SHA-256 |
| --- | --- |
| xml-0.2.1.zip | `96b4e933259ffe3155d1967256da2d2882c4104ebe0535cf8297f7599d083ab9` |
| html-0.3.0.zip | `9282282f148b229979b2b2a9eff806eedba79f9ca8377091b409b3b85f330962` |
| javascript-0.3.0.zip | `01208c40c187fea2e558e36c43e2234c07de58ac968320a50db567016716ade6` |
| capability-example-0.17.0.zip | `f3f5ffa4b76e565f3a5d29bb989ee120a3923d88faae92b57137a857ac109dab` |

本轮使用重新构建的 Debug 宿主提供 SDK 与测试入口，插件组件使用 Release 构建。其余 ignored 服务、安装器与平台完整矩阵未重跑；macOS/Linux 未构建或实测。原生交互结果来自真实包的 GPUI 帧与输入夹具，不作为本轮人工桌面启动验收或 Release 安装器重验的证据。

其他分支检查、候选导出和原始内容备份见上述本机日志目录；后续工作区清理不删除已发布的分支提交。主分支合并推送后继续核对各原始工作区对应的 12 个分支与远端 SHA，并检查现存工作区状态。
