# 02 — 编辑未保存 Markdown 并实时查看原生分栏预览

**Status:** completed — 实际插件原生预览、独立能力夹具及双轴审查通过，见[验收记录](../verification/02-native-preview.md)。远端交付读回随下一阶段记录。

**GitHub:** [#28](https://github.com/T-miracle/Editor/issues/28)；父方案 [#26](https://github.com/T-miracle/Editor/issues/26)。

**规格：** [Markdown 插件总方案](../spec.md)。验收覆盖：M02（预览）、M04（基础分栏）、M06、M14、M15、M16。

## What to build

打开 Markdown 默认显示可拖动的左源码、右预览，常用语法随内存文档实时更新。

## Acceptance criteria

- [x] 通过通用编辑区预览贡献接入原生分栏，源码仍使用既有 EditorState 与 DocumentSession。
- [x] CommonMark 基础语法、GFM 表格、任务列表和删除线可见；任务框本单只读，图片可先显示替代文字，代码块先以等宽文本呈现。
- [x] 输入、粘贴、撤销、重做和重新加载更新预览；文档身份、revision、实例校验拒绝过期结果。
- [x] 解析结果保留源码块范围与渲染块对应信息，供后续交互和滚动复用；不维护第二份可变文档。
- [x] 切换非 Markdown 文档、禁用或卸载后收回分栏；中英文、深浅主题及原生布局通过实际包 GPUI 验证。

## Blocked by

- 01 — 安装插件后识别并高亮 Markdown

## Implementation guardrails

先核对现有组合 UI 与 SVG 预览接缝；必要的小范围预重构先保持现有 SVG 行为并验证，再接入本单功能。不能假设现有原生文本节点已提供完整富文本排版。

遵守插件根部 AI 执行约定。所需接口、权限、宿主接入、插件行为、SDK 与可观察回归在本单闭环；必要预重构先保持行为并验证，不能以占位 UI 或仅协议声明完成工单。保留其他任务改动，新增代码同步补注释。

## Verification

沿真实插件包、公开管理器、原生编辑器交互主接缝验证以上验收项。先运行针对性回归，再执行仓库要求的格式、非 UI workspace 测试与 workspace 编译；涉及 editor-app 追加相关测试和原生验收，实际 WASM 测试先构建再显式运行。记录失败归因与未验证内容，不以 mocked 内部结果替代完整行为。
