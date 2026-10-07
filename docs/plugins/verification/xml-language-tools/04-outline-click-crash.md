# 大纲点击崩溃修复

日期：2026-10-08（Asia/Shanghai）。范围为用户实际查看示例时报告的“大纲定位点击闪退”，来源为[工单 03](../../tickets/xml-language-tools/03-outline-and-docking.md)及[方案](../../specs/xml-language-tools.md)的原生大纲交互。首次交付记录保留，不能用旧验收代替本次回归。

状态：标题栏崩溃已通过稳定 RED → GREEN、共同门禁及独立两轴审查。实际 XML 导航验证另捕获解析取消后撤销的独立异常，正在处理；不能把该失败记为导航验收通过。修复宿主在后续异常处理完成后重新启动。

## 现场与反馈循环

- 启动版本基线：`faf4cbe6eecf150e867dccbf177345759fbd0108`，隔离工作树 `codex/xml-language-tools`。
- 用户窗口日志：`target/svg-smoke-466eecca0b4742dca786810e82c2a8ed/interactive-fb97964aa2c4413e96b2f931e5f07c63-stderr.log`。主线程实际错误为 `cannot update editor_app::outline::panel::OutlinePanel while it is already being updated`，随后跨 Windows 原生回调不能 unwind，进程退出。
- 捕获调用链为大纲标题栏隐藏按钮 → `EditorApp::toggle_outline` → 再次更新同一个 `OutlinePanel`。大纲行定义跳转和标题栏按钮是不同的点击路径；本次不能仅重跑此前树行跳转测试便声称覆盖现场。
- 最小场景是不安装语言工具、不加载 WASM 的真实 GPUI 窗口；点击跟随按钮再点击隐藏按钮。仍产生完全相同的面板重复借用错误，排除语言服务、插件结构和 SVG 渲染作为本次崩溃所必需的条件。

反馈命令：

```powershell
# 使用本工作树 target/native、既定 MSVC LIB 和 RUST_MIN_STACK 的实际原生测试。
cargo test -p editor-app --no-default-features native_outline_header_buttons_keep_window_alive -- --test-threads=1 --nocapture
```

首次 RED：0 passed、1 failed，测试 0.08 s，含首次编译合计 65.13 s；相同内容再次 RED：0 passed、1 failed，测试 0.09 s，命令合计 1.00 s。两次均为上述相同面板错误，不是附近的另一失败。

最小修复后的同一测试 GREEN：1 passed、0 failed，测试 0.10 s，含编译合计 50.99 s。点击跟随按钮改变实际偏好，点击隐藏按钮撤去实际面板控件，窗口继续可用。日志为 `target/xml-language-tools/outline-header-crash-{red,red-confirmation,green}.log`。

## 根因与修复

优先核对的三项假设为标题栏事件持有面板时重复借用、Dock 隐藏机制重入、插件数据异常。无插件最小场景已排除第三项；只改变标题栏隐藏回调的上下文后同一测试变绿，确认第一项。

隐藏按钮原先使用 `cx.listener`，回调期间独占借用 `OutlinePanel`；回调立即更新父 `EditorApp`，父动作又通知并更新该面板，违反 GPUI 的实体借用约束。改为捕获既有弱父句柄的 App 级按钮回调，不借用无需修改的面板实体；保留正常隐藏动作、事件停止冒泡、工作区偏好和 Dock 位置。不延迟或吞掉错误，不改变导航范围、文本、插件契约或生命周期。

新增原生回归位于 `crates/editor-app/src/tests/outline/chrome.rs`，使用现有按钮测量和真实 `simulate_click` 接缝；未增加测试专用宿主 API。标题按钮沿用公开本地控件，补充实际 hit region 的稳定 debug selector；源码注释说明为何不能借用面板。

## 最终验证与交付

共同门禁对标题栏修复内容只运行一次；真实 XML 包、SDK 和服务内容未改变，不重复 Schema、格式化、标签和四向 Dock 全部矩阵。实际 XML 包的大纲定位、跟随、编辑和撤销回归单独运行，其余 ignored 未运行项不计通过。

| 命令 | 实际结果 |
| --- | --- |
| `cargo fmt --check` | PASS，2.06 s |
| `cargo test --workspace --exclude editor-app` | 202 passed、0 failed、173 ignored，12.62 s |
| `cargo check --workspace` | PASS，2.90 s |
| `cargo test -p editor-app --no-default-features -- --test-threads=1` | 376 passed、0 failed、162 ignored，命令 162.51 s、测试 150.27 s |
| `cargo build -p editor-app` | PASS，34.24 s；既有警告保留 |
| `git diff --check` | PASS |

两轴审查比较基线 `faf4cbe6eecf150e867dccbf177345759fbd0108` 与固定候选 `e6eff1f6c796270b020f7cf0d9d6c187706a3a14`。Standards：硬性违规 0、判断性结论 0、最严重问题无；Spec：缺失 0、范围蔓延 0、错误实现 0、最严重问题无。随后只补充本段实际验证结果，没有改变受审源码。

实际 XML 原生回归 `native_outline_package_navigation_follow_and_revocation` 两次失败（42.35 s、47.36 s）：大纲行跳转已完成，但第二次原生 `Ctrl+Z` 在 `SyntaxHighlighter::update_edits` 空解析的 `unwrap` 抛错。第二次日志带调用栈，位于 `target/xml-language-tools/outline-crash-xml-native-navigation-backtrace.log`；其实体、错误和触发步骤与标题栏重复借用不同。后续修复另行提交和记录，不吞掉此失败，不据此撤销已通过的标题栏回归证据。

Windows x86_64 为本次实际平台；macOS/Linux 未验收。
