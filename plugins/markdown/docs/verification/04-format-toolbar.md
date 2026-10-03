# 04 — 原生格式工具栏验收

日期：2026-10-04。代码基线：`4f2c9bd1411c9b64c4058a74d1db016e39e15387`。Markdown 包版本：`0.4.0`；独立 SDK 夹具：`capability-example 0.15.3`。对应工单 [#30](https://github.com/T-miracle/Editor/issues/30)。状态：行为验收、必需检查和双轴审查通过。

## 实现与边界

包通过公开 `editor.toolbar 1.0` 贡献源码顶部五组共 13 个格式按钮，包含中英文提示。工具栏自动调整高度；真实 120 px 源分栏中全部按钮边界可见且不重叠。预览模式隐藏源码和工具栏；源码模式仍能显示同一公共树的 Dialog / Menu，不产生第二个模态实例。通用 Canvas SVG 复用普通视图的工作线程栅格缓存与原生投送。

公开 `editor.edit 1.0` 提供带文档身份与 revision 的选区读取、单次范围替换和编辑回执。范围使用半开 UTF-8 字节坐标，替换可携带预期选区，宿主检查活跃身份、版本、权限、中文 IME、范围和取消状态。原生 EditorState 的 Atomic replace 和 Change 订阅是唯一写入、DocumentSession 版本与撤销来源；请求按 GPUI effects 顺序串行，下一条请求在原生版本更新后执行。

插件只保留宿主只读快照和异步意图，不维护另一份可变文本或撤销栈。新意图和源版本变化取消旧任务。块命令按完整行处理、保持 CRLF、排除恰在下一行开头结束的行；代码使用比正文反引号更长的围栏。强调保留边缘空白、逐行包裹有效正文；链接与图片转义标签中的反斜杠和方括号。生成的强调和引用使用预览相同的解析器核对完整源文的精确元素范围，无法表达的边界明确拒绝并显示双语原因，不插入不可见字符或扩大选区。

图片按钮本单只插入引用模板；图片加载、粘贴和拖入分别归 05 / 06。普通预览仍只读，任务写回归 07。不存在 Markdown 专属宿主分支。

## 实际验证

隔离工作树 `codex/markdown-plugin`，独立 target；环境设置 `CARGO_BUILD_JOBS=1`、`RUST_MIN_STACK=16777216`。

- `./target/debug/editor-app.exe --plugin-cargo plugins/markdown/Cargo.toml test -- --nocapture`：17 passed、0 ignored；覆盖 13 命令选区与模板、中文、换行、围栏、CRLF、非法 UTF-8、强调空白与完整上下文、引用转义与跨段拒绝。测试纯搬迁至 `format/tests.rs` 后路径与名称保持。
- `./scripts/build-plugins.ps1 -Packages markdown -HostExe target/debug/editor-app.exe`：最终 0.4.0 组件和 ZIP 构建通过。
- `cargo test -p editor-app extensions::markdown_tests -- --ignored --nocapture`：最终真实 ZIP 7 passed、0 ignored。覆盖 13 个按钮实际编辑与单次撤销、重做、模板输入、120 px 原生拖动、完整 Tab / Space 与新意图取消旧意图；原预览和模式回归；同批旧 revision 读写拒绝、UTF-8、选区竞态、中文 preedit、取消和退休实例；独立 SDK Source-only Dialog 中文输入 / Escape、Menu / Escape、工具栏 SVG 与禁用释放。
- `crates/plugin-runtime/tests/editor_edit.rs` 的 10 个真实 WASM 用例沿开发步骤显式分批运行：全部通过，0 ignored；检查协商、独立能力、匹配读写授权、工作区及面板归属、共享嵌套能力、1 MiB 配额、取消、退休、Source-bound 点击与模态优先级。最终分批结果在 `target/edit-contract-logs/{01-green,02-green,03-green-repeat,04-green,05-green,06-green,runtime-regressions}.log`；旧运行时编辑请求 1 passed，protocol 14 passed。
- `cargo test -p editor-app ui::plugin::tests -- --nocapture`：12 passed；原控件点击、禁用、键盘、输入、中文 IME、主题角色、滚动和弹窗焦点仍通过。
- `cargo test -p editor-app svg_preview_follows_open_documents -- --nocapture`：1 passed。
- `cargo test -p editor-app typed_editor_requests_read_selection_and_save -- --ignored --nocapture`：1 passed、0 ignored；原版本化选区与保存不切换文档。
- `cargo fmt --check`、`cargo test --workspace --exclude editor-app`、`cargo check --workspace`：通过；其他未显式运行的 ignored 测试仍是跳过，不计为通过。访客源文件另用 Rust 2024 rustfmt 检查。
- `cargo build -p editor-app`、`./scripts/verify-plugin-sdk.ps1 -HostExe target/debug/editor-app.exe`：最终公共 SDK 导出、缓存修复及仓库外独立组件构建通过，包含新契约与文档。
- `git diff --check`：通过。

## 审查修正与失败归因

真实包先复现同批旧版本写入通过、仅编辑模式工具栏首次发布不刷新 dock、模态不可见、工具栏 SVG 缺失以及窄分栏组内裁剪；公开宿主接缝修复后均通过。纯规则测试先复现强调边缘空白、标点上下文以及引用标签失效，使用实际解析事件验证修正。初始重做与拖动测试分别改为项目 Windows 的 Ctrl+Y 和完整原生 drag 生命周期，没有修改产品键位或伪造布局。

早期配额夹具尝试通过资产字节数组传递大文本，超过执行燃料；已撤销该路径，采用独立访客有界 `repeat_text` 构造测试请求，没有提高执行预算。历史名称带 green 的失败日志不算通过，采用最终 `03-green-repeat.log`。

Standards 与 Spec 独立审查最终均无剩余问题。既有 Wasmtime LNK4217 和未使用接口提示保留；未把编译、跳过或其他插件历史记录当作本插件行为验收。非 Windows 交互未执行。
