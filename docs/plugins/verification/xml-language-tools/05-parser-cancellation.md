# 解析取消后的原生撤销回归

日期：2026-10-08（Asia/Shanghai）。本次是[大纲点击崩溃复验](04-outline-click-crash.md)发现的第二个独立失败，保持[方案](../../specs/xml-language-tools.md)规定的唯一原生编辑状态、动态 grammar 和过期结果校验。没有改变插件声明、包、SDK 或权限。

状态：最小原生反馈、完整 XML 场景、共同门禁及独立两轴审查均通过，修复宿主已重新打开并保持窗口存活。此记录不替代标题栏重复借用的独立证据。

## 已捕获失败

实际命令：

```powershell
# 使用已构建的实际 XML 0.2.0 ZIP、既定 target/native 和 MSVC 环境。
cargo test -p editor-app --no-default-features native_outline_package_navigation_follow_and_revocation -- --ignored --test-threads=1 --nocapture
```

两次均为 0 passed、1 failed，测试分别 42.35 s、47.36 s。后一轮设置 `RUST_BACKTRACE=1`，调用链为原生 `Ctrl+Z` → EditorState 历史重放 → `InputHighlighter::update` → 大纲折叠适配器 → `SyntaxHighlighter::update_edits`；错误发生在空解析返回值的 `unwrap`，不是 `OutlinePanel` 重复借用。日志为 `target/xml-language-tools/outline-crash-xml-native-navigation{,-backtrace}.log`。

## 假设与验证方向

测试恢复策略前按以下顺序核对：

1. 同步 2 ms 取消后复用解析器。如果正确，确定性取消后立即原生撤销会复现，重建取消的解析器后消失。
2. 后台 100 ms 取消的候选被安装。如果正确，取消结果必须拒绝发布；仅取消待发布任务不能修复它的解析器状态。
3. XML grammar 自身无法解析空文本。如果正确，从未取消的新解析器也会失败；取消前的完整解析和新解析器可以区分它。

锁定版本 `tree-sitter 0.26.13` 的公开 Rust 文档要求在回调取消后改解析其他输入前 `Parser::reset()`。锁定 `gpui-component 0.7.0` 的取消分支没有调用 reset，其 Parser 字段也没有公开恢复接口。宿主通过公开 `SyntaxHighlighter::new` 重建，不修改依赖源码，不引入内建 grammar，不吞掉 panic。

## 最小反馈循环与修复

定向探针仅记录解析取消和候选发布，统一使用 `[DEBUG-xml-cancel]`。原始实际包第三次仍失败（45.40 s）；唯一取消记录为同步解析 1624 字节、generation 5，异常发生前没有后台完成或发布记录。这将现场原因缩小到同步取消后复用，不把后台失败当成已经复现的原因。

最小窗口移除 Manager、结构请求、树导航和停靠流程，仅通过现有动态 XML grammar 及公开 InputHighlighter 工厂驱动真实原生编辑器。独立零预算探针确认 grammar 的取消回调可达；但零预算适配器、较短源文本等删减场景均通过，不能冒充现场 RED。恢复现场的 127 字节 XML 基线、1624 字节粘贴、实际 2 ms 时限和原生绘制后，立即 `Ctrl+Z` 稳定产生相同 `highlighter.rs:637` 错误。

```powershell
# 不需要原生语言服务器；使用现有实际动态 XML grammar 夹具。
cargo test -p editor-app --no-default-features native_xml_cancelled_parser_preserves_paste_undo_redo -- --test-threads=1 --nocapture
```

首次 RED：0 passed、1 failed，测试 2.86 s，含编译 24.19 s。原样缓存再跑 RED：0 passed、1 failed，测试 2.88 s，命令 3.82 s。错误、原生 Undo 栈和取消记录相同；日志为 `target/xml-language-tools/outline-parser-cancel-red-native-budget{,-confirmation}.log`。

取消后将当前高亮器标记为只读，仅保留缓存绘制，不再调用它解析新文本；新的完整解析在既有后台机制执行，避免在 UI 线程重建 WASM 解析器。后台返回 false 的候选丢弃，只有成功结果通过 generation、文本和 grammar epoch 校验后才替换并解除取消状态。原生 EditorState、磁盘和 Undo/Redo 状态没有复制或绕过。

同一最小原生测试 GREEN：1 passed、0 failed，测试 1.11 s，含编译 57.64 s；检查原生粘贴、一次撤销、重做、磁盘保持原文及继续绘制。源码为 `crates/editor-app/src/outline/folding/cancellation.rs`。测试使用正常生产 2 ms 策略，已移除探索阶段的私有时限参数和全部定向日志；没有新增测试专用宿主 API。

## 最终验证与交付

最终源码冻结后执行下列检查。只计实际运行的 ignored 场景；SDK、插件资源和服务未变，不重跑无关分发、Schema 及全部 Dock 矩阵。

| 命令 | 实际结果 |
| --- | --- |
| `cargo fmt --check` | PASS，1.93 s |
| `cargo test -p editor-app --no-default-features native_outline_package_navigation_follow_and_revocation -- --ignored --test-threads=1 --nocapture` | 1 passed、0 failed，测试 42.61 s、含编译 60.98 s |
| `cargo test --workspace --exclude editor-app` | 202 passed、0 failed、173 ignored，10.68 s |
| `cargo check --workspace` | PASS，2.26 s |
| `cargo test -p editor-app --no-default-features -- --test-threads=1` | 377 passed、0 failed、162 ignored，测试 127.16 s、命令 133.81 s；包含两项新增崩溃回归 |
| `cargo build -p editor-app` | PASS，26.86 s；既有警告保留 |
| `git diff --check` | PASS |

完整实际 XML 包场景重新验证行定义定位及焦点、跟随、中文未保存输入、评论折叠、粘贴与撤销、深色主题与 DPI、切换文件、关闭重开和停用撤销。此前三次原始失败不覆盖或删除；本轮完整命令已经 GREEN。

两轴审查比较基线 `0288ada912ae0930032251321a04c5bfb8608804` 与固定候选 `09fd8ce98c8fd83454c4c5b733dd3042462e198c`。Standards：硬性违规 0、判断性结论 0、最严重问题无；Spec：缺失 0、范围蔓延 0、错误实现 0、最严重问题无。两份受审 Rust 文件与最终构建源码的 blob 身份一致；受审后仅补充实际验证与启动记录，没有改变源码。

修复版 `target/native/debug/editor-app.exe` 的 SHA-256 为 `3D0E78B9AB8329A6631D3B4E64B461CCE214271C32A725AE0D3530184EFE4440`。通过既有示例私有插件目录重新打开 `target/svg-smoke-466eecca0b4742dca786810e82c2a8ed/workspace/xml-demo.svg`，恢复该示例会话的大纲可见性，保留文件、其他布局偏好和原来的 XML/SVG 包。实际进程 PID `22808`、原生窗口句柄 `3149552`，2026-10-08 01:29:26 启动（Asia/Shanghai），随后读回仍存活，stdout/stderr 为空；窗口留给用户继续操作。启动日志为同示例目录的 `repaired-56dd233c465044558cd23415f83025e7-{stdout,stderr}.log`。交互证据来自上表真实 GPUI 点击及输入测试，启动检查不代替交互验收。

Windows x86_64 为本次验证平台，macOS/Linux 未验收。
