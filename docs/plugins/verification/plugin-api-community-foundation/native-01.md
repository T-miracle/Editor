# 01：主代理 Windows 原生验收记录

日期：2026-10-09。状态：原生发现项已修复并完成受影响路径复核；工单交付仍以逐单门禁和双轴审查为准。

## 首轮输入

- 宿主副本：`C:/Users/Tmiracle/.codex/worktrees/plugin-api-community/native-qa-01/bin/editor-app.exe`。
- 宿主 SHA256：`2b4ae1837f39fb17cc320aae608535f7494f46c6435fe6c3b91854b615eb61ae`。
- 内嵌 SDK：`98b5137b113d5890ad0e63f26caa3d24587761ad1ce5bef3580fd4f2f2d16e4c`。
- 插件：`history-preview` 0.1.0，经该宿主 `--plugin-dev` 构建和实际激活；候选摘要 `45ddd8eb3bd01dcb61f09552ddb77dde9e880ec61729c61dbcef8c6a5144d07a`。
- 隔离目录：工作树内 `target/native01-profile-root` 与 `target/native01-workspace`，不使用日常配置或用户文档。
- 授权参数：`--grant editor.read --grant ui.panels`。
- 原生操作使用 Computer Use 的 Windows 输入、截图及 UI Automation；没有通过测试专用宿主接口调用比较。

## 已观察行为

1. 从插件面板菜单执行 `Compare historical content`，打开带“只读”标记的历史快照 Tab，呈现历史内容与 `current.txt` 的原生双栏及差异标记。中文、emoji 与 CRLF 文本可见。
2. 在右栏选择全部并输入 `未保存的右侧文本😀`、`second changed line`，本地文档标记 dirty。原比较因版本变化关闭，再次执行比较后右栏呈现最新未保存内容。
3. 点击左栏后按 Ctrl+S，消息窗口显示“此文档为只读内容，无法保存”。右栏仍为 dirty。磁盘文件操作前后 SHA256 均为 `6b1ecbe68f1053447688cb8da0426c49fbed2d8ad0571870a81c9188cf793749`，没有保存右栏或创建虚拟文件。
4. 左栏 Ctrl+A 可选择文字，输入中文及 emoji 不改变历史内容；选择和编辑权限分别成立。
5. 设置窗口切换深色主题后，主窗口、右栏与插件面板更新；左栏背景更新，但行号背景和当前行仍使用浅色样式，列为待修复。

## 首轮发现与复核要求

- 打开虚拟 Tab 后，插件运行状态出现管理器 `os error 123`。实施代理已定位到 bundled first-use 发现把虚拟 URI 作为本地路径送入文件系统。必须补真实 worker 回归，并在新候选原生窗口中确认不再出现此错误。
- 比较左栏没有同步主题设置。必须复用现有编辑器样式入口，同步两侧行号、选区、当前行、字体与差异颜色，再复核深浅主题。
- 比较绑定精确版本，源版本变化关闭，需要再次比较；该规则须在 SDK 中英文站点说明中准确表达。
- 后续候选源码、SDK 或包变化后记录新 hash，不能把本轮存在发现项的二进制当作最终通过。

控制器已通过 `stop` 正常关闭其自有实例。后续原生输入使用新的隔离运行数据。

## 修复候选复核

- 宿主副本 `native-qa-01/bin/editor-app-final.exe` SHA256：`9d645c003758a3fed88effc94e7a7fed3102e2e6fbc6376dab5d58c715d94853`；SDK 仍为 `98b5137b113d5890ad0e63f26caa3d24587761ad1ce5bef3580fd4f2f2d16e4c`。
- 新配置 `native-qa-01/profile-history-final`，新工作区 `native-qa-01/workspace-final`。当前 SDK 独立构建的 `history-preview` 开发候选：`5bba5b5a6b977d7bc6411ad258fbddb35be7f216ceb766d1a2c3cf3a4b0d1836`。
- 初始本地文件为 UTF-8 中文、emoji、CRLF；操作前后磁盘 SHA256 均为 `ae9dfed7dac311fb8daa75098df7bf349baf13526e7d4b6aee58df111cb9c1e6`。

1. 通过历史插件菜单打开双栏，左侧 Ctrl+S 显示“此文档为只读内容，无法保存”；没有写入磁盘或虚拟文件。
2. 从浅色切换深色后，稳定画面中两侧行号、当前行、背景与差异标记均采用深色样式，左侧没有首轮的白色残留。
3. 保持虚拟 Tab 打开，查看本次 History Preview 运行日志，仅见安装完成和忽略旧 UI 迟到回调；未再出现首轮 `os error 123`。真实 worker 对虚拟身份的负面回归另见逐单记录。
4. 右栏 Ctrl+A 输入 `未保存的右侧文本😀` 和 `changed line` 后，本地 Tab 标记 dirty，原比较按 revision 失效关闭。执行 **Refresh historical content** 后重新比较，左栏显示“历史版本已刷新😀”，右栏显示最新未保存文本。
5. UI Automation 在普通编辑画面能读到编辑节点，但比较打开时未暴露两侧区域、编辑节点或关闭按钮。该轮基础无障碍语义仍待修复与原生复核，不能将颜色通过当作 C03 全部完成。

这一候选的控制器已正常 `stop`。后续只重验受修复影响的原生路径，并另行验证生成预览消费者；最终状态以候选指纹、逐单结果和双轴审查为准。

## 无障碍候选与生成预览

候选源码 `3a0d34561a3a4d60928ac66185ca3b4b17059429`，宿主副本 `native-qa-01/bin/editor-app-accessible.exe` SHA256 `fc63bcaca64a55d55111ce0671493395ead47fbc412051f09a27fc5d07311b27`；内嵌 SDK `b9b4caa36115776248eaf7c2e213a9a720fb4f056388927b6f3a637f996baea3`。正式包分别为 history-preview 0.1.0 `85ac2463994f7812785f9622e6ec234c46e12aadff477d91c510e18a65c03897` 和 generated-preview 0.1.0 `29634b6cc753c97ea65c8ad50441dc15bc5541514dbb8abe0a8298e1ccb504aa`。

直接执行宿主 `--plugin-dev plugins/generated-preview --profile ../native-qa-01/profile-generated-final --workspace ../native-qa-01/workspace-final --grant editor.read --grant ui.panels --grant workspace.read`，开发候选 `716fbb0d7d7c1a2aef96ef4e4702ab3a74f7b202a0c3739348ce5ebc7e199fab`。使用新的独立配置；测试文件与上一轮工作区相同，磁盘 hash 保持 `ae9dfed7dac311fb8daa75098df7bf349baf13526e7d4b6aee58df111cb9c1e6`。

1. 在当前原生编辑器输入未保存的“未保存的生成预览😀”及英文 `unsaved generated line`，Tab 标为 dirty。通过插件菜单执行 **Preview generated text**，左栏显示“生成的建议内容”、同一未保存中文与 `UNSAVED GENERATED LINE`，右栏保持原始未保存文本；没有把磁盘内容当作插件快照。
2. Windows UI Automation 实际返回左侧 `Document` 节点：名称含“历史 / 生成内容 · 生成文本预览 · 只读”，Value 含完整建议内容；未声明 settable。右侧为带“当前文档 · current.txt”来源区域的 settable Edit，关闭按钮名为“关闭比较”。这修复了上一轮比较画面不暴露节点的问题。
3. 点击左栏后输入“只读输入应被拒绝”，文本与 Value 均未改变；Ctrl+S 明确提示“此文档为只读内容，无法保存”，右栏仍 dirty、磁盘 hash 未改变。
4. 从浅色切换深色后，稳定左栏行号、当前行、背景和差异均使用深色，没有首轮白色残留。右栏的近黑行号背景与当前行在关闭比较后的普通原生编辑器中同样存在，属于现有右栏样式；本单不把两栏像素完全相同列为已验收结果。
5. **尚未通过：** 左栏焦点下点击“关闭比较”可移除双栏和差异标记，但后续 Ctrl+A 不选择当前文本，UIA 焦点落在窗口。这暴露了关闭按钮未把键盘焦点还给当前编辑器；已准备真实按钮点击回归，修复后需用新候选重验，不提前关闭工单。

本轮控制器已通过 `stop` 正常结束自有开发实例。已通过的文本、只读和辅助技术观察仅对应上面的候选指纹；最终焦点和退役结论另行追加。

## 审查修复候选：关闭焦点与实例退出

宿主副本 `native-qa-01/bin/editor-app-review-fix.exe` SHA256 `95a3332aa41ef23eb8ad0437027144a73a0b852855b67bd27b2de6eeed30de83`；内嵌 SDK `df7c52db8ecd3696a8cb4891af75d85d7e1b7a58e91fc8da7b7616375cb5e066`。正式独立包已同步提升至 0.1.1：history-preview `8f147cf36dca8056f1c4e3319490d8da768fa1aaa6b8856055ab5f3fb255e200`，generated-preview `a77fcc34f3cc9c44a00a9d319250ecd3acd69712e31108d0e05a7ceacdc46bc6`，均通过宿主入口构建。

本次使用 `profile-review-fix`、`workspace-review-fix`，以相同开发入口和授权激活 generated-preview。仅复核审查修改影响的关闭、只读、辅助技术和实例生命周期路径；未重新宣称上一候选的全部主题操作在此二进制上人工执行。

1. 普通编辑器 Ctrl+A 输入“未保存的生成预览😀”，再通过 Return 输入 `unsaved generated line`，Tab 标为 dirty。实际菜单 **Preview generated text** 打开左侧建议内容与右侧未保存原文。
2. UIA 返回名称包含来源及只读状态的左侧 `Document`、完整 Value，右侧 settable `Edit` 和名称为“关闭比较”的 Button。左侧点击后键入“只读输入应被拒绝”未改变文本，Ctrl+S 给出只读警告；磁盘 hash 仍为 `ae9dfed7dac311fb8daa75098df7bf349baf13526e7d4b6aee58df111cb9c1e6`。
3. **焦点修复通过：** 保持左侧焦点，直接点击关闭按钮，双栏消失。不重新点击编辑器，Ctrl+A 在屏幕实际全选右侧两行；接着输入“关闭后键盘输入已恢复😀”，普通编辑器的可见文本与 UIA Value 同步改变。只检查 UIA 的窗口焦点标签不足以证明编辑器失去焦点，本次以真实键盘选择、输入和 GPUI FocusHandle 回归共同判定。
4. 首次启动遗漏 `tty=true`，工具 stdin 已关闭，无法发送开发控制器的 `stop`。已按本次独有可执行路径核对并清理自有控制器及子进程；这次清理不计为正常退出验收。
5. 随后使用同一二进制、新的 `profile-review-stop` 及 `tty=true` 重启，实际菜单打开比较。发送 `stop` 后控制器退出码为 0；精确可执行路径的进程列表为空，窗口列表不再包含该自有窗口。磁盘 hash 不变。未关闭或控制桌面上的其他验收实例。

本轮原生测试数据保持隔离，文本没有保存到磁盘。成功替换、禁用、信任撤销、失败候选保留和原有 readonly/其他装饰及非编辑器焦点的保留，由实际 SDK 包驱动的 GPUI 生命周期矩阵负责，详见逐单验收记录。

原生退出后仅完善两包内 README 的可执行构建示例并重新归档，最终 ZIP 为 history-preview 0.1.1 `793af3015f7a8a7aee8668d1c2947380f6cfe75b90693e00434fb17a79c4d25e`、generated-preview 0.1.1 `6adddb8976f33fdc059dc7e8c2ec62be3f4fd66495d1259a56a17674ca7a8d2c`。ZIP 内 WASM 的 SHA256 前后相同，分别为 `d2d7e0d53e4eea0f89e6ad4b95174ee06464b88275f6782f26e8c8deb65d5623` 和 `1d24504aaf87d14f4287632db37729b20e0a62fd19dc02366000cfa20012f214`；宿主、SDK、清单与资源行为未变。本轮操作没有使用修改后的 README 作为输入，复用上述原生观察，并在最终实际包回归中使用新的 ZIP。

## Spec 复审追加的自动退役边界

对 `3ef99c5d8b82f946c7c2a2688ddae4a3ad8cfe7c` 的固定提交复审中，Standards 未解决项为 0；Spec 确认原三项已修复，另发现左比较栏正持有焦点时，自动退役清理没有转移焦点。GPUI 未挂载焦点的默认路径仅通知监听器，而现有宿主没有注册恢复逻辑。这是静态路径结论，不能写成已经执行失败的原生操作。

随后在现有实际 SDK 生命周期矩阵中增加 trust-loss 时的左侧点击、退役后的右侧 Ctrl+A 与中文输入；保留另两段对无关控件焦点的检查。红测确实失败于撤下左栏后右编辑器未获焦点，修复提交 `9a242a7f6448dc5b6989aa464b4d7a2ea687329e` 后绿测为 1 passed、0 failed、0 ignored。上面 `95a333…` 的显式 Close 证据不替代该自动退役分支。

## 自动退役修复候选：真实重载与键盘恢复

宿主副本 `native-qa-01/bin/editor-app-auto-focus.exe` SHA256 为 `bbec2a401ae92fe94018730481f6b2cc660907f0e01d2458945dd719a49c0109`，对应 `9a242a7f`。SDK 摘要仍为 `df7c52db8ecd3696a8cb4891af75d85d7e1b7a58e91fc8da7b7616375cb5e066`；两份正式 0.1.1 ZIP 与 WASM 均未变化，沿用上一节最终摘要。

使用新的 `profile-auto-focus`、`workspace-auto-focus`，通过 `--plugin-dev plugins/generated-preview` 和原有三项 grant 启动；开发控制器使用 `tty=true`，日志为 `native-qa-01/generated-auto-focus.log`。本次只复核新焦点分支及实际实例退出。

1. 在普通编辑器输入未保存的“退役前未保存😀”与 `reload keeps buffer`，实际菜单打开比较。左侧 Document Value 为“生成的建议内容”、同一中文和 `RELOAD KEEPS BUFFER`，右侧保持未保存原文。
2. 点击左比较栏后发送控制器 `reload`。重载准备期间，实际 UIA focused element 为左侧只读 Document，证明这次自动清理开始前左栏确实持有焦点；日志随后记录 generation 1 准入并激活。
3. 成功替换撤下旧比较与虚拟 Tab，仅保留仍 dirty 的本地文档。不重新点击右编辑器，Ctrl+A 在屏幕全选两行；输入“自动退役后输入正常😀”后，可见文本与 UIA Edit Value 同步变为该内容。
4. `current.txt` 操作前后磁盘 SHA256 都为 `ae9dfed7dac311fb8daa75098df7bf349baf13526e7d4b6aee58df111cb9c1e6`。右侧输入没有保存到磁盘。
5. 发送 `stop` 后控制器退出码 0，精确自有可执行路径的进程列表为空。未控制或关闭其他验收窗口。

本轮物理输入证明虚拟比较的成功 dev reload 路径；local→local 的信任撤销、禁用与无关控件焦点保留由前述实际 SDK/GPUI 生命周期矩阵验证，没有把它们写成已逐项人工操作。固定范围 `3ef99c5d…9a242a7f` 的最终 Standards 与 Spec 复审分别为 0 个剩余可操作发现。
