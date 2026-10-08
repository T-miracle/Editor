# 大纲单一高亮与临时路径展开

日期：2026-10-08（Asia/Shanghai）。本记录补充工单 03 的原生大纲交互，不改变插件包、公开协议、权限或父议题状态。

## 已确认行为

- 仅编辑区光标所在的最内层节点绘制选中背景；大纲的键盘/鼠标浏览选中仍由 Base 管理，不额外绘制第二处背景。
- 默认显示根节点及其直接子节点两层。开启跟随时临时展开当前节点的祖先，移动到另一位置后收起旧路径；没有深层目标时恢复两层。
- 关闭跟随后保留手动展开、键盘导航及同一文档修订后的浏览状态。

## 反馈与修复

原生回归命令：`cargo test -p editor-app --no-default-features native_outline_single_highlight_and_temporary_follow_expansion -- --test-threads=1`。

RED：0 passed、1 failed，测试 0.19 s；实际绘制 2 个选中背景，旧自动展开分支仍可见，回到第二层后深层节点仍可见。测试从结构快照进入真实宿主模型，移动原生编辑器光标并读取绘制矩形及可见节点；不以内部缓存值代替界面结果，也不依赖语言工具。

GREEN：相同测试 1 passed、0 failed，测试 0.18 s。行背景只使用当前光标节点；跟随更新重建两层基线与当前祖先路径。原有目标、revision、提供者租约校验及唯一编辑状态保持不变。

实际 XML 包命令：`cargo test -p editor-app --no-default-features native_outline_package_navigation_follow_and_revocation -- --ignored --test-threads=1`。使用已构建的 XML ZIP，1 passed、0 failed、0 ignored，43.56 s；包括原生跳转、展开、可关闭跟随、手动展开跨修订保留、撤销、主题/DPI 及提供者撤销。

`cargo fmt --check`、`cargo test --workspace --exclude editor-app`、`cargo check --workspace` 与 `git diff --check` 均通过。没有重跑无关插件的 Schema、服务依赖或 SDK 打包；保留现有编译警告。

## 后续反馈：示例中没有生效

用户再次反馈临时展开/收起没有生效。检查本任务使用的示例工作区会话，发现 `outline_follow_cursor` 保存为 `false`。原先标题按钮使用统一 ghost 外观后，开启与关闭没有视觉区别，提示也只写“跟随光标”，无法看出实际保存状态。

将原生回归改为真实 Ctrl+Home/方向键和鼠标点击，通过实际标题按钮从关闭切换到开启；不再直接设置光标或通知应用来驱动跟随。修改外观前，真实方向键回归已经通过（1 passed、0 failed，1.34 s），因此没有将编辑器观察者改动作为修复。开启时的展开/收起算法在该输入路径正常工作。

修复标题状态辨识：开启用主题强调色图标、关闭用弱化色图标，按钮仍保持资源管理器一致的无填充背景；中英文提示和无障碍名称明确给出已开启/已关闭及点击操作。保留可关闭的手动浏览方式与现有工作区偏好，不重置其他会话。

补充鼠标点击后，`cargo test -p editor-app --no-default-features native_outline_ -- --test-threads=1`：2 passed、0 failed、2 ignored，0.92 s。两项通过分别覆盖单一高亮/真实输入展开收起与标题/底栏操作；两项 ignored 是实际插件包验收，不计入通过。

随后显式运行实际 XML 包的 `native_outline_package_navigation_follow_and_revocation -- --ignored --test-threads=1`：1 passed、0 failed、0 ignored，43.75 s。本次将原先直接设定深层光标并通知应用的步骤替换为点击真实源码位置，再用 Ctrl+Home/方向键移至根节点，断言深层节点消失且第二层仍可见；重新点击深层位置可再次展开。该路径使用实际独立 XML WASM 包及公开管理器，没有宿主测试专用接口。

本次 `cargo fmt --check`、`cargo test --workspace --exclude editor-app`、`cargo check --workspace`、`git diff --check` 均通过；保留现有警告。

优化 release 构建通过（1 min 12 s）。仅将本任务示例工作区的跟随偏好恢复为开启，并运行 `target/native/release/outline-follow-state-preview/editor-app.exe`；进程创建了原生窗口，启动标准输出/错误无内容。运行证据只确认新版可启动，鼠标/键盘展开收起的行为证据以上述原生回归及实际 XML 包测试为准。

## 后续简化：移除标题定位图标

按用户最新要求移除大纲标题栏的定位/跟随按钮及其未使用的中英文提示；标题只保留隐藏按钮。光标归属、高亮、两层基线、临时路径展开与工作区跟随偏好保持原有实现。

原生回归 `cargo test -p editor-app --no-default-features native_outline_ -- --test-threads=1`：2 passed、0 failed、2 ignored，0.90 s。标题检查确认定位按钮不存在，隐藏面板及底栏恢复正常；模型检查保留真实方向键、鼠标点击、单一背景及临时分支收起验收。两项需要实际包的 ignored 验收没有重复运行，不计为本次通过。

`cargo fmt --check`、`cargo test --workspace --exclude editor-app`、`cargo check --workspace` 与 `git diff --check` 均通过；保留已有编译警告。本次为标题控件删除，没有变更插件包或公开契约。

优化 release 构建通过（1 min 11 s），产物为 `target/native/release/outline-header-preview/editor-app.exe`；后续示例启动入口优先使用此版本。本轮未启动窗口。

## 后续默认值：隐藏大纲

新工作区的 `outline_visible` 默认改为 `false`；旧会话缺少该字段时也按隐藏读取。已有明确保存的显隐值仍按工作区恢复，底栏窗口控制栏按钮继续负责打开/隐藏。本任务示例会话已设为隐藏，其他会话没有重置。

原生标题/跟随回归：2 passed、0 failed、2 ignored，0.92 s，包含初始标题不可见、底栏显式打开、标题隐藏及底栏恢复。会话回归：4 passed、0 failed、0 ignored，0.30 s，覆盖新默认值、旧记录缺字段、显隐保存恢复和停靠布局恢复。

原有延迟插件停靠测试假定大纲默认可见，改为明确保存显示偏好的迁移夹具，仍验证完整等比例布局及插件状态，未放宽原来的布局断言。实际包验收通过底栏显式打开大纲，不再依赖默认显示。

显式执行实际 XML 包原生验收（`native_outline_package_navigation_follow_and_revocation -- --ignored --test-threads=1`）：1 passed、0 failed、0 ignored，43.76 s。`cargo fmt --check`、`cargo test --workspace --exclude editor-app`、`cargo check --workspace` 和 `git diff --check` 均通过，保留既有警告。

优化 release 构建通过（1 min 12 s），产物为 `target/native/release/outline-hidden-preview/editor-app.exe`，后续示例启动入口已优先选择该版本。本轮未启动窗口。
