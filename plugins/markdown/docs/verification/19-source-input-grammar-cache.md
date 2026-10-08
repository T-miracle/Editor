# 0.15.0 后续：源码输入延迟的宿主根因（插件 grammar 每次解析重新编译）

日期：2026-10-05。承接 [17](17-incremental-input-viewport.md) 与 [18](18-source-input-and-stale-viewport.md)：那两轮把预览发布、增量视图、布局与视口通知都处理过一遍，但用户实测“仅编辑（无预览）”仍有明显延迟，中文输入法尤其明显。本次定位到**宿主侧与 Markdown 包无关的根因**并修复；Markdown 包行为、协议与版本保持 0.15.0，不改访客逻辑。未提交、未推送、未关闭工单。

## 用户可见缺陷与测量口径

- 现象：Markdown 源码编辑区每次输入（含输入法组字更新与提交）都有明显卡顿；仅编辑模式同样存在，与预览是否可见、格式工具栏是否显示无关。
- 口径：`window.dispatch_keystroke` 在编辑路径内的同步耗时（`edit`）、其后同步跑完的观察者（`effects`）、随后一帧（`draw`）。夹具为真实 ZIP 经公开 Manager 进入宿主（`markdown_tests` 既有 harness），文档为 300 段正文 + 60 行 GFM 表格 + 围栏代码块。

## 根因

宿主注册给 GPUI Kit 的 grammar parser 工厂（[crates/editor-app/src/language/plugins.rs](../../../../crates/editor-app/src/language/plugins.rs)）每次调用都新建 `WasmStore` 并 `load_language`，而 Tree-sitter 的 WASM 支持在**每个 store** 上重新编译模块（`ts_wasm_store_load_language` → `wasmtime_module_new`，`ts_wasm_store_new` 还会再编译一次 stdlib）。上游 highlighter 对**每个注入层、每次解析**都通过 `LanguageRegistry::parser()` 取一个新 parser，因此 Markdown 源码每敲一个字都会走一次完整编译：注入 grammar `markdown_inline` 是插件 WASM（426 KB）。

实测（同一台机器，`plugins/markdown/grammar/*.wasm`，`WasmStore::new` + `load_language` 各一次）：

| 项目 | debug | release |
| --- | --- | --- |
| `WasmStore::new`（stdlib 编译） | 85–91 ms | 7–8 ms |
| `load_language(markdown)` | 1.22–1.24 s | 82–87 ms |
| `load_language(markdown_inline)` | 1.31–1.33 s | 87–89 ms |

编辑器端到端（release，长文档，仅编辑 + 工具栏）：每次输入 `edit` = **92.6–98.9 ms**；文档长短、是否有工具栏都只改变其余数毫秒，`edit` 恒定在 ~93 ms，与上表一次 store+grammar 装载吻合。同文档在无 grammar（`text` 高亮）下 `edit` = 26–100 µs。

这也解释了此前“大文档反而没那么糟”的观测：同步解析先撞上上游 2 ms 预算超时后，注入解析被挪到后台线程，CPU 仍被占用但不阻塞输入；release 下解析足够快、能在预算内完成，于是每次输入都在 UI 线程付一次编译。

## 修改

- `crates/editor-app/src/language/plugins.rs`：插件 grammar 共用一个**带编译缓存的 Wasmtime 引擎**（`grammar_engine()`／`build_grammar_engine()`）。首次装载仍会编译，之后每个新 store 的模块装载变成缓存命中；每个 parser 仍持有自己隔离的 store，解析语义不变。
- 缓存位置：`dirs::cache_dir()/MeEditor/grammars`；打开失败只记警告并退回默认引擎，不影响插件加载。校验路径（后台 worker）也走同一引擎，因此首次输入前缓存已在后台预热。
- `crates/editor-app/Cargo.toml`：为启用该缓存增加直接依赖 `wasmtime = { version = "36.0.16", default-features = false, features = ["cache"] }`；代码仍使用 `tree-sitter` 重导出的类型，避免版本分叉。
- 回归测试：`language::plugins::tests::repeated_grammar_loads_reuse_compiled_modules`（私有临时缓存目录，第一次为真实编译、第二次必须命中）。绕过缓存时该测试失败并打印 `first 96.1682ms, second 89.5328ms`，修复后通过，证明它守得住这个缺陷。

## 结果

同一探针、同一夹具，release：

| 配置 | 修复前 `edit` | 修复后 `edit` | 修复后 input→draw |
| --- | --- | --- | --- |
| 短文档 + markdown grammar | 92.6–98.9 ms | 2.16–2.56 ms | — |
| 长文档 + markdown grammar | 93.3–96.7 ms | 5.43–7.96 ms | 6.45–6.57 ms（无工具栏）/ 8.87 ms（有工具栏） |
| 长文档 + 无 grammar | 0.03–0.10 ms | 不变 | — |

grammar 装载（store + 模块）本身：release 88→**1.5 ms**（≈58x），debug 1.3 s→**8.4 ms**。

剩余量级：release 下每次输入约 5–9 ms，其中约 2 ms 是上游同步解析预算、约 1–2 ms 注入层解析、其余为编辑器布局与一帧绘制；格式工具栏在首帧后约 +0.4–2.3 ms（debug 下曾测得约 +20 ms，属上游“每个 revision 重投影工具栏”的开销，未在本次改动范围内）。

## 验证

- 定向：`cargo test -p editor-app language::plugins::tests`（debug 6 passed、release 6 passed，含新回归测试）。
- 反向验证：临时关闭缓存 → `repeated_grammar_loads_reuse_compiled_modules` FAILED（`first 96.1682ms, second 89.5328ms`）→ 恢复后通过。
- 真实包 ignored 套件（`--ignored --test-threads=1`）：`input_latency` 1、`responsiveness` 4、`modes` 2、`first_open` 1、`preview_updates` 3、`synchronized_scroll` 6、`format_toolbar` 4、`code_highlighting` 3、`source_layout` 2、`distribution` 10 全部通过。
- 门禁：`cargo fmt --check`、`cargo test --workspace --exclude editor-app`、`cargo check --workspace` 通过；`cargo test -p editor-app -- --test-threads=1` 251 passed / 0 failed / 116 ignored。
- 失败归因（与本次改动无关，已逐项排除）：
  - `markdown_tests::task_checkboxes` 3 项、`link_navigation` 3 项失败。把本次改动临时旁路（退回 `Engine::default()`）后同样失败，失败断言属于预览任务框回写与链接打开计数，与 grammar 编译无关；对应文件本轮正由另一会话在工作区中改动。
  - `cargo test -p editor-app`（默认并发）出现 10–15 项顺序相关失败（含 `explorer::menu`、`app::plugins` 等与 grammar 无关的用例），串行执行全绿，属既有测试隔离问题。

## 未验证与限制

- 未做操作系统中文输入法候选窗的人工验收：样例仍为原生字符按键与探针测量，需在实际文件、物理键盘、真实输入法下复测。
- 发行包已按用户要求重建：`cargo build -p editor-app --release` → `dist/editor/editor-app.exe`（95,479,808 字节，SHA-256 `2FA6CA0319FCB555FB9366AD643FDF946E369039872A4F7475B4604744A82532`，与 `target/release/editor-app.exe` 一致）。`dist/editor/plugins/markdown.zip` 与 `bundle-defaults.json` 未重新打包：Markdown 包本轮无改动，其源码仍在另一会话未收敛的改动中（`task_checkboxes` / `link_navigation` 回归未修复），重新打包会把在途改动一并交付。新二进制经 CLI 路径冒烟（`--plugin-cargo` 缺参返回清晰错误、无窗口进程残留）。
- 注入口语仍是“每层一次 parser”：宿主只能让每次装载变便宜，无法让上游复用常驻 parser；若上游改为按语言复用 parser，这 ~1.5 ms 也可省去。
