# 01 — 语言包验证记录

日期：2026-10-03。比较基线：`9740f7c`。插件版本：`0.1.0`。环境：Windows、Rust 1.95、Tree-sitter WASM ABI 15。

实现、针对性测试、必需检查及 Standards / Spec 双轴审查通过。对应工单 [#27](https://github.com/T-miracle/Editor/issues/27)。

## 实际验证

- `cargo test -p editor-app markdown_language_package_restores -- --nocapture`：1 项通过，真实 ZIP 经 Manager 安装，已打开文档由 text 恢复 Markdown；标题、粗体、斜体、代码、删除线、链接产生非默认样式。禁用、启用和卸载同步切换语言并撤销旧 grammar 名称。
- `cargo test -p editor-app installed_injection_queries_cannot_borrow_undeclared_native_grammars -- --nocapture`：1 项通过。独立 novel 语言真实 ZIP 验证静态未声明、动态 capture 和重复目标均被拒绝发布。
- `cargo fmt --check`、`cargo test --workspace --exclude editor-app`、`cargo check --workspace`、`git diff --check`：均通过。非 UI 测试中的既有 ignored 夹具仍为跳过，本单没有以它们代替上述真实 grammar 验证。
- `./scripts/build-plugins.ps1 -Packages markdown -HostExe target/debug/editor-app.exe`：生成 `dist/plugins/markdown.zip`。实际 HostExe 使用已有宿主可执行文件的绝对路径；资源包无需编译空 WASM 组件。
- 初次共享缓存 check 读到另一工作区旧 schema；改为本工作树独立 target 并清理该缓存中的 plugin-schema 产物后通过。首次并行链接出现 LNK1102 内存不足，设置 `CARGO_BUILD_JOBS=1` 后完整非 UI 测试通过。没有降低测试范围。

## 审查修正

- 将区块与行内 grammar 通过公开声明式注入组合。不能仅凭语言名确认成功，断言实际着色范围。
- 合法行内 grammar 不接受空文本，不能以空树含错误拒绝安装；仍检查模块、ABI、查询编译及 parser bridge。
- 上游 registry 未实施注入白名单，因此通用加载边界要求每个 pattern 恰好一个静态、显式声明目标；缺失或禁用的提供者保持纯文本。
- 不恢复宿主 Markdown 扩展名回退，不增加插件专属 API。

本单仅交付语言资源包。原生实时预览及完整发行验收在后续工单实施，不能以本单结果宣称全部功能完成。
