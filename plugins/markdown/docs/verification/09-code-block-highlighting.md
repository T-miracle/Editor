# 09 — 代码块高亮验证记录

状态：行为验收通过。基线 `ae7b9f375a129e17791df37e66c6c3d49bc90778`。2026-10-04 读回 #35 open / ready-for-agent、直接前置 #28 closed / completed；08 已推送且 #34 closed / completed。Git 交付与关闭随后独立读回。

本单采用声明式 `ui.code_highlighting 1.0` 与 `Document.code_highlighting`。原有 CodeBlock 的 language/text 在精确 source 版本下构成只读请求；使用用户选中的动态 WASM 提供者及当前主题，不建第二份可变文本、Undo 或代码块 LSP，不查询内建 grammar。后台结果绑定 source、scene、node/text、owner、provider 与不重置的 epoch，撤销或失败即时降级等宽文字。

真实主接缝 RED：0.8 ZIP + 公开安装独立 novel-code-primary（实际 TOML WASM）+ 当前原生代码块，文本、语言、源身份、内存与磁盘断言都通过，唯一失败为未绘制非空 captures 的 StyledText 行；21.16 秒，target/09-native-code-red.log。初稿 Option/Result 编译修正不计功能 RED。

公开 opt-in 无 source 的协议行为 RED：旧版将新字段忽略而 validate 成功；增加字段及 source 门禁后 1 passed，target/09-protocol-{red,green}.log。首轮仅字段序列化形状断言不算该 source 行为 RED。

## 实现与实际包验证

Markdown 0.9.0、独立 capability-example 0.15.7。公开能力同时要求 `ui.code_highlighting`、`ui.richtext`、`editor.read` 和自有工作区编辑器面板；即使根节点是 Text 也不绕过权限。提供者、静态注入语言和源文档共享不可重置的 epoch，过期、取消、关闭、替换和配额失败均不应用迟到结果。捕获名称、原始／归一化字节、注入深度、区间、查询事件及解析层使用共同有限预算。

| 实际命令／路径 | 结果 | 日志 |
| --- | --- | --- |
| `cargo test -p plugin-protocol` | 26 passed；0 ignored | 09-protocol-tests.log |
| `cargo test -p editor-app language::code_highlighting -- --test-threads=1` | 12 passed；34.73 秒 | 09-language-final-green.log |
| `cargo test -p plugin-runtime --test code_highlighting -- --ignored --test-threads=1` | 9 passed；218.23 秒 | 09-runtime-code-final.log |
| 宿主 `--plugin-cargo plugins/markdown/Cargo.toml test` | 53 passed | 09-guest-tests.log |
| 原生 tracks_provider_choice_replacement_and_plain_fallback、uses_an_independent_enabled_language_provider | 2 项在最终 ZIP 下通过 | 09-native-code-complete.log |
| 原生 delivered_markdown_code_keeps_source_undo_and_rejects_retired_document_scenes，单项 `--ignored --test-threads=1` | 1 passed；33.34 秒 | 09-native-source-final.log |
| `cargo test -p editor-app ui::plugin -- --test-threads=1` | 33 passed；1.95 秒 | 09-native-ui-regression.log |
| `cargo build -p editor-app`；`./scripts/verify-plugin-sdk.ps1` | 宿主及独立 SDK 消费者成功 | 09-final-host-build.log、09-final-sdk-verification.log |
| `./scripts/build-plugins.ps1 -Packages markdown -HostExe ./target/debug/editor-app.exe` | 真实包成功；15.42 秒 | 09-final-markdown-build.log |
| `cargo test -p plugin-runtime --test sdk_distribution -- --ignored --test-threads=1` | 1 passed；17.61 秒 | 09-final-sdk-smoke.log |

日志位于未入库的 target/。最终 SDK 摘要 `4b5e959f7b7e3b0dbfadac6831614a69dac7d376af29edb2db9708aec00fee20`；最终 Markdown ZIP SHA-256 `aa5e09aa951638c3c826d1b00fb08ff68a4bb344fa995a924ea567036b4c2343`。SDK 独立构建和原生测试均使用最终包；原生 3 项由两次运行共同覆盖，不把其中一次含失败的整套运行写成全部通过。

## 回归、失败归因与审查

原生实际包验证用户选择的主／替代提供者、安装、替换、停用、重启用、当前主题、未知／无语言标记降级、中文 CRLF 精确区间、剪贴板修改一次撤销、保存关闭及重开新文档身份。注入沿实际 Markdown WASM 验证独立内联提供者及撤销，不恢复宿主 grammar 或启动额外 LSP。

审查发现的缺少注入、捕获名称预算及 Text 根节点能力绕过均先验证行为 RED，再修复并通过回归。实际 AST 诊断发现默认注入会剔除匿名标点；包查询显式 include-children 保留粗体标记，临时诊断代码已移除。早期原生夹具误将多次键盘输入当成一次 Undo，并在仍为 dirty 的会话尝试关闭；改为真实 Ctrl+V 单次编辑和现有 SaveDocument 原生动作后通过，不改写编辑器撤销或脏状态语义。孤立测试窗口没有主程序 Ctrl+S 绑定，故不将无效键绑定实验计为通过。

`cargo fmt --check`、Markdown／独立示例访客格式检查及 `cargo check --workspace` 通过（最终编译 3.15 秒）；`cargo test --workspace --exclude editor-app` 实际 73 passed、105 ignored、37 个结果组（09-workspace-tests.log）。忽略项不计为通过，本单涉及的实际 WASM 已显式执行。文档链接、包查询与 ZIP 一致性及 `git diff --check` 已核对。

独立 Spec 最终 0 findings；Standards 最终硬规则 0、Fowler 判断性 0，修复与最终日志均已独立复核。Windows 原生 GPUI 自动交互是本单验收平台，不冒称额外人工桌面验收。保留环境已有链接及未使用接口警告；同步滚动和发行继续由 10–11 交付，父议题 #26 不修改。

2026-10-04 交付：普通提交 `783498a9697354ca9a4505048d57a42def491529` 已推送 origin/codex/markdown-plugin，远端 SHA 一致；#35 PATCH 后独立 GET 读回 closed / completed。此记录追加在 10，不改写已推送历史。
