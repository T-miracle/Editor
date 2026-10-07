# 资源管理器文件转移验收记录

日期：2026-10-07。工作分支：`codex/explorer-file-transfer`；基线：`main` 的 `ae63225afd6e8a362c6f186d7374323e97405469`。

规格：[explorer-file-transfer](../specs/explorer-file-transfer.md)。实施顺序：[外部粘贴 #78](https://github.com/T-miracle/Editor/issues/78)、[拖放 #79](https://github.com/T-miracle/Editor/issues/79)、[撤销重做 #80](https://github.com/T-miracle/Editor/issues/80)。

状态：实施中；未完成的检查与原生验收不标记为通过。

## 测试入口

以已挂载 EditorApp 的资源树菜单、键盘和拖放事件为入口，检查临时工作区的真实磁盘与原有标签状态。Windows 系统剪贴板及原生拖入另行验收，不以合成事件替代原生协议验证。

## 已执行

- 首个回归测试 `explorer_transfer_paste_conflict_keeps_both` 在基线上失败：同名粘贴没有冲突选择。
- 第一阶段 `cargo test -p editor-app explorer_transfer`：10 项通过，覆盖剪切保留未保存会话、快捷键焦点、同名选择、合并、脏目标保护、失败与取消、Windows 联接跳过及删除失败清理。
- 第一阶段 `cargo fmt --check`、`cargo check --workspace`、`cargo test --workspace --exclude editor-app` 均通过。
- Windows 原生验收：在系统文件资源管理器中对本任务临时样例执行 Ctrl+C / Ctrl+X，再在资源树执行 Ctrl+V。复制保留源文件，剪切移除源文件，目录原有内容保留，树更新正常。验收仅涉及 `target/explorer-native/` 下的临时样例。

## 待执行

- 树内拖动、修饰键、外部多项拖入、悬停展开、边缘滚动、取消恢复。
- 撤销、重做、后续磁盘冲突、未保存内容二次确认、焦点与会话清理。
- 必需的格式、workspace 测试和编译检查，以及分阶段针对性 GPUI 测试。
- Windows 真实系统文件管理器输入与明暗主题检查。
- macOS/Linux 支持范围与未运行验证项的记录。

## 实现参数

- 保留两份沿用原扩展名，依次选择 `名称 (2).扩展名`、`名称 (3).扩展名` 等空闲名称。
- 后台操作超过 350ms 显示进度。
- 文件操作错误使用本地 Notification，直到手动关闭；定位提示仍为 1.2s。
- 会话备份使用项目目录之外的私有临时目录；不承诺跨重启恢复。
