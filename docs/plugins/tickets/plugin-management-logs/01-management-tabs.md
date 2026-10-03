# 01 — 分栏浏览插件说明与运行信息

**Status:** completed — 已完成实现与验收（2026-10-03）。GitHub 交付状态由提交推送后读回核对。
GitHub：[#23](https://github.com/T-miracle/Editor/issues/23)；父方案：[#22](https://github.com/T-miracle/Editor/issues/22)。
规格：[插件管理与运行日志改造](../../specs/plugin-management-logs.md)。

## What to build

用户打开插件管理即可默认阅读概览，切换到独立运行日志页查看已有服务状态与诊断，滚动内容时仍能操作固定顶部的重启和启用选项。本切片从原有状态来源接入可见日志区域，完整运行期历史由下一切片扩展。

## Acceptance criteria

- [x] 六个下划线 Tab 按规定顺序呈现，默认概览；中间四项置灰且不能被鼠标或键盘激活。
- [x] 名称、版本、操作和 Tab 固定，仅下方内容滚动；README 留在概览，现有运行信息和错误移入日志区域。
- [x] 重启按钮位于全局选项前，重启可正常执行；安装、更新、卸载和启用范围语义保持。
- [x] 左侧搜索与 Tab 距离、本机安装区域内边距缩小，点击区域和焦点行为正常。
- [x] 中英文、深浅主题、长内容、缩放和键盘交互通过验证。

## Blocked by

None — can start immediately.

## Testing Decisions

覆盖 U01–U04 的布局与已有数据展示部分、U12；实际插件安装后经原生管理入口操作并读取可见结果。使用现有原生插件管理及重启测试接缝，完成针对性 editor-app 测试与仓库阶段检查。

## Implementation guardrails

复用本地控件及既有状态发布；必要布局抽取先保持行为并验证。不新增专属测试 API；生成代码同时生成注释。

## Delivery

[验证记录](../../verification/plugin-management-tabs-verification.md)。完整运行历史、未读图标及正常过期回调的提醒处理仍由 #24 实施；底栏 A 方案由 #25 实施。
