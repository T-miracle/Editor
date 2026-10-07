# 资源管理器文件转移验收记录

日期：2026-10-07。工作分支：`codex/explorer-file-transfer`；基线：`main` 的 `ae63225afd6e8a362c6f186d7374323e97405469`。

规格：[explorer-file-transfer](../specs/explorer-file-transfer.md)。实施顺序：[外部粘贴 #78](https://github.com/T-miracle/Editor/issues/78)、[拖放 #79](https://github.com/T-miracle/Editor/issues/79)、[撤销重做 #80](https://github.com/T-miracle/Editor/issues/80)。

状态：代码与自动验证已完成，待 Windows 原生联合验收；未完成的检查不标记为通过。

## 测试入口

以已挂载 EditorApp 的资源树菜单、键盘和拖放事件为入口，检查临时工作区的真实磁盘与原有标签状态。Windows 系统剪贴板及原生拖入另行验收，不以合成事件替代原生协议验证。

## 已执行

- 首个回归测试 `explorer_transfer_paste_conflict_keeps_both` 在基线上失败：同名粘贴没有冲突选择。
- 第一阶段 `cargo test -p editor-app explorer_transfer`：10 项通过，覆盖剪切保留未保存会话、快捷键焦点、同名选择、合并、脏目标保护、失败与取消、Windows 联接跳过及删除失败清理。
- 第一阶段 `cargo fmt --check`、`cargo check --workspace`、`cargo test --workspace --exclude editor-app` 均通过。
- Windows 原生验收：在系统文件资源管理器中对本任务临时样例执行 Ctrl+C / Ctrl+X，再在资源树执行 Ctrl+V。复制保留源文件，剪切移除源文件，目录原有内容保留，树更新正常。验收仅涉及 `target/explorer-native/` 下的临时样例。
- 第二阶段 `cargo test -p editor-app explorer_transfer_`：13 项通过，新增树内移动与 Ctrl 复制、外部多项拖到文件父目录、599/600ms 悬停展开、边缘滚动和 Escape 恢复临时展开。首个拖动测试在实现前失败。
- 第二阶段格式、非 UI workspace 测试和 workspace 编译检查通过；真实原生拖入与主题检查留到第三阶段合并验收。
- 第三阶段针对性文件操作测试已通过 18 项，新增菜单撤销部分成功记录的用例也在完整套件中通过。覆盖复制、目录移动、合并、路径占用跳过、强制恢复的未保存内容二次确认及重做新冲突。
- 第三阶段格式、非 UI workspace 测试和 workspace 编译检查通过。`cargo build -p editor-app` 通过。
- 完整 UI 套件并行运行：369 通过、15 失败、142 ignored。失败包含既有共享语言/插件注册状态的用例及菜单标签查找；同一版本以 `--test-threads=1` 串行复验为 384 通过、0 失败、142 ignored。ignored 夹具未在本次任务中构建和运行。
- 上一轮审查修复后的完整 UI 套件串行运行：390 通过、0 失败、142 ignored。随后新增退出清理测试通过；该次 `cargo test -p editor-app explorer_transfer_ -- --test-threads=1` 为 26 通过、0 失败，`cargo test -p editor-app leave -- --test-threads=1` 为 5 通过、1 ignored（既有 LSP 夹具未构建）。
- `7c3b964` 对应的 `cargo fmt --check`、`cargo check --workspace`、`cargo build -p editor-app`、`git diff --check` 通过；非 UI workspace 测试在该轮修复后通过。最后的退出与备份释放修复另按下条记录验证，当前原生验收窗口尚未载入这些最后修复。
- 最后复核新增两项回归：强制撤销移动遇到未保存占用标签，以及 Quit action 退出时仍在等待冲突选择。两项均先失败后通过；最终资源管理器针对性测试为 28 通过、0 失败。会话测试 3 通过，退出相关测试 5 通过、1 ignored。格式、非 UI workspace 测试与 workspace 编译检查再次通过；未重复完整 UI 套件。
- 站点 `npm test`：15 通过、2 跳过（新工作区没有搜索构建产物）；本次仅修改双语资源管理器使用说明。

## 待执行

- Windows 系统文件管理器跨窗口原生拖入，等待人工操作结果；自动化 API 不允许跨窗口 drag，相关越界调用未执行输入。
- Windows 原生“外部导入 → 树内移动 → 撤销 → 重做”联合场景、明暗主题反馈及最终版本的原生退出清理。验收窗口与样例保留在 `target/explorer-native/`，不操作其他任务的编辑器窗口。
- macOS/Linux 原生运行验收没有环境，本轮不宣称通过；实际支持范围见下文。

## 审查修复

以 `git diff main...HEAD` 沿 Standards 与 Spec 分别审查。首轮 Standards 4 项、Spec 4 项，其中普通覆盖授权的空窗为两轴共同发现；逐项处理，不合并两轴结论。

- 磁盘准备、完整内容校验、私有备份和递归清理留在后台。最终发布在一个 UI 回调内检查当前未保存状态、会话身份及 revision，仅执行同卷重命名和标签路径迁移。失败通过重命名回滚，避免在 UI 复制大备份；回滚失败保留停车原件并记录实际变化。
- 强制恢复的丢弃授权绑定 `file_id`、文档 revision 与 capability revision；关闭重开或确认后新输入使旧授权失效，异步读取还检查打开/保存时间。
- 目录恢复维护由已检查状态与本次实际发布结果推导的内容树，拒绝确认期间新增文件；新目录容器不会删除用户跳过的子项，整目录替换可正常撤销、重做。
- 工作区路径重新解析 canonical root，拒绝父目录被联接替换后的重定向。
- 拖动落点使用本帧实际可见行的裁剪布局，自动滚动重绘和释放时重新命中；离开目标失效旧悬停计时，返回后重新开始 600ms。
- 新增回归测试分别复现目录替换无法重做、确认后输入被覆盖、返回目录无法展开，再验证修复。25 项文件操作测试通过，另覆盖目录确认后变动、边缘滚动释放和父目录联接替换。
- 正常窗口退出显式取消后台工作、等待其完成并清理本会话私有备份，避免依赖进程退出的析构。实际粘贴生成备份的退出回归测试通过，重复关闭不能绕过未完成清理，也不删除项目、来源或无关临时内容；Windows 联合验收正在执行。
- 强制移动恢复遇到未保存的占用标签时，最终身份与 revision 校验通过后先关闭该占用会话，再迁移原文档；保持原编辑器身份。新输入仍会使旧丢弃授权失效。
- 原生窗口关闭与 `QuitEditor` 共享同一个清理任务；先取消活动工作、释放冲突等待，等待已启动的历史备份后台释放，再清理资源登记表。关闭期间菜单和恢复入口禁止新任务，晚到结果保留到登记路径已清理后，不与登记表并发删除。
- 新操作废弃 redo、空结果和未保留记录时，在后台释放最后的备份引用，避免 UI 递归删除大型备份。

最终代码提交为 `ae26fa963e32d46c7417d654f11447515b55b393`。两个独立审查均已按 `git diff main...HEAD` 确认：Standards 0 项硬性违规、0 项需处理的判断性坏味道；Spec 0 项剩余代码规格发现。审查通过不代表下述原生验收已完成。

## 平台与原生验收边界

- Windows 原生剪贴板复制/剪切已验收；系统文件管理器跨窗口拖入仍需真实操作验收。当前 computer-use API 的 drag 限定在单一窗口内，越界调用被拒绝，不能用 GPUI 合成拖入冒充 OS 协议通过。
- macOS 使用 GPUI 原生路径及文件 URI 输入，文件 URI 的 `cut` 标记可表示移动；未实现 Finder 原生剪切意图。
- Linux 使用文件 URI 输入与公共原生拖入入口，原生文件剪贴板的支持依赖 GPUI 后端。
- macOS/Linux 未运行原生 UI 验收，未安装额外工具链；Windows 结果不代表其他平台通过。

## 实现参数

- 保留两份沿用原扩展名，依次选择 `名称 (2).扩展名`、`名称 (3).扩展名` 等空闲名称。
- 后台操作超过 350ms 显示进度。
- 文件操作错误使用本地 Notification，直到手动关闭；定位提示仍为 1.2s。
- 会话备份使用项目目录之外的私有临时目录；不承诺跨重启恢复。
