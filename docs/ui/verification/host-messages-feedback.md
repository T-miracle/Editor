# 宿主消息窗口运行反馈修正

日期：2026-10-08（运行截图反馈来自 2026-10-07）

依据：[现行规格](../specs/host-messages.md)、[当前来源盘点](host-messages-sources.md)，以及用户在原生窗口预览后提供的标注截图和 IDEA 消息范围补充。

起点：已交付的 `370eaa49fe1123546702ae4b624140d167f5c7a1`，工作区 `C:/Users/Tmiracle/.codex/worktrees/host-messages/Editor`，分支 `codex/host-messages`。原始 #85、#86 验收及关闭记录保留；不新增 Git 产品能力，不修改父设计议题。

## 本次范围

- 清空按钮改为本地 Icon 绘制的目录扫帚 SVG，保留 Base Button 的焦点、键盘激活、空列表禁用、中英文悬浮提示与无障碍名称。
- 消息按钮进入左下窗口工具组，按现有窗口/功能组分隔；预留一个固定位置，插件窗口溢出也不隐藏通知入口及红点。
- 底部取消操作状态文字，仅保留窗口工具、诊断/插件指示及光标位置。内部状态仍用于既有流程，避免改变文档与刷新行为。
- 日常文件打开/关闭/保存、正常磁盘同步、文件树操作成功、刷新、切换主题、普通配置保存/新增/删除成功不再发布历史。警告、错误及异常目标的显式修复结果保留；必要 Info 没有被全局过滤。
- 单列表、当前运行 500 条、首批及追加 20 条、清空与确认时序、布局恢复、插件日志隔离保持原要求。

IDEA 参考仅用于确定“重要事件和待处理提示”的内容取舍，不增加它的分组、建议专区或通知设置页。参考：[JetBrains 官方通知文档](https://www.jetbrains.com/help/idea/notifications.html)。

## 自动化验证

统一环境：`CARGO_TARGET_DIR=C:/Projects/RustProjects/Editor/target`，`RUST_MIN_STACK=16777216`。

- 新增真实来源回归 `host_messages_routine_operations_stay_quiet`。最初夹具编译失败（访问文件树私有输入字段），改用已聚焦原生输入后，正确红阶段编译成功并在“打开文档应为空”断言失败；修改生产来源后单测通过。
- `cargo test -p editor-app host_messages -- --nocapture`：**9 passed / 0 failed / 0 ignored**。除新来源回归外复用原有容量、展开、滚动、清空、红点、并发到达、布局、受限工作区及插件日志隔离场景，未增加重复夹具。
- 单个源策略场景执行真实打开、编辑和保存并核对磁盘字节，使用原生输入创建文件，切换主题、刷新并关闭文档后仍为空；随后真实打开失败产生错误与红点，重要 Info 仍可进入列表且不确认错误。
- 现有图标资源验证纳入 Broom，验证其打包加载与清单路径；没有新增仅断言图标名称的重复测试。

- `cargo fmt --check`：通过。
- 首次非 UI workspace 测试因 `LINK : fatal error LNK1104: 无法打开文件“msvcrt.lib”` 未执行完成。已有 MSVC 库实际存在，本次检查进程设置 `LIB` 为 MSVC 14.50.35717 的 `lib/x64` 和 Windows SDK 10.0.26100.0 的 `ucrt/x64`、`um/x64` 后重试；未安装工具或修改全局环境。

- `cargo test --workspace --exclude editor-app` 重试：**196 passed / 0 failed / 167 ignored**。跳过项需要额外 WASM/外部服务夹具，未报为通过。
- `cargo check --workspace`、`cargo build -p editor-app`：通过。编译器/链接器既有 warning 保留，没有扩大修复范围。
- 第一次 `cargo test -p editor-app` 默认并行运行：**355 passed / 19 failed / 142 ignored**。语言注册表测试相互覆盖并导致 provider 锁中毒，另有共享菜单状态断言失败；原验收记录使用 `--test-threads=1`。按相同内容和既有单线程参数复验，未修改无关实现或测试断言以掩盖失败。

- `cargo test -p editor-app -- --test-threads=1`：**374 passed / 0 failed / 142 ignored**，166.55 秒。默认并行失败的语言、菜单及高亮回归在单线程中均通过；不额外修改共享全局测试隔离。142 项额外 WASM/外部夹具测试未执行，不计通过。

## Windows 原生交互

程序为本工作区构建后立即复制的 `C:/Users/Tmiracle/.codex/worktrees/host-messages/acceptance/host-messages-refined.exe`，SHA-256 `4AD787B99233B2F8E1D2713EA699DD2DE05FE4DA99C7C56DDF566A1D5F1A97A0`。用独立临时工作区及空插件根验收，不编辑用户项目文件。

1. 启动真实文本文件：右侧为空；左下窗口工具组显示 Explorer 与通知图标；右侧底部仅有光标位置，左侧没有“已打开”文字。标题栏是小扫帚，没有常驻清空文字。
2. 点击编辑面输入“【验收输入】”：文档变脏，消息仍为空。实际点击标签关闭：宿主警告保留，左下通知图标出现红点，编辑文档保留。
3. 点击扫帚：警告与红点同时清除。重新聚焦编辑区并用 Ctrl+S 保存，读取临时文件确认输入落盘；列表仍为空，底部没有保存成功文字。
4. 收起消息窗口并关闭干净文档：编辑区域扩展，日常关闭不产生红点。点击文件树定位当前文件，在无活动文档条件下真实产生 Warning；窗口仍隐藏、图标显示红点。点击左下通知按钮恢复面板，已有红点消失，警告记录仍在。
5. 通过实际设置按钮切换深色主题：通知、扫帚及警告图标均可辨认；主题切换没有追加消息。清空、键盘、中英文、滚动、缩放和接收时序的组合回归由统一 GPUI 场景覆盖，未重复进行全部旧工单人工步骤。

双轴审查及普通提交/推送结果待填。
