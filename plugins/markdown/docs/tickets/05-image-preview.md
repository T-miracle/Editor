# 05 — 预览本地与获授权网络图片

**Status:** passed — 2026-10-04 实际包资源、原生图片和纹理回收、SDK 独立构建及 workspace 检查通过，独立 Standards／Spec 审查均无剩余问题。Git 交付与 issue 关闭读回补入[验收记录](../verification/05-image-preview.md)。

**GitHub:** [#31](https://github.com/T-miracle/Editor/issues/31)；父方案 [#26](https://github.com/T-miracle/Editor/issues/26)。

**规格：** [Markdown 插件总方案](../spec.md)。验收覆盖：M08、M10（读取边界）、M06、M14、M16。

## What to build

Markdown 中的本地相对图片和网络图片能够显示，失败时给出替代文字与原因。

## Acceptance criteria

- [x] 本地图片相对当前 Markdown 文档目录解析，经规范化路径与符号链接校验，不跨授权工作区边界读取。
- [x] 网络图片通过获授权的受控能力加载；无权限、网络错误及解码失败有可见替代文字与原因。
- [x] 加载完成更新预览块高度与布局信息；慢请求在切换文档或新 revision 后不能覆盖当前预览。
- [x] 停用、卸载和关闭文档时取消或作废图片任务并回收资源，不在 UI 线程执行阻塞下载。
- [x] 以隔离文件和受控网络响应验证成功、失败、越界、取消及迟到结果，明确实际支持的图片格式。

## Blocked by

- 02 — 编辑未保存 Markdown 并实时查看原生分栏预览

## Implementation guardrails

不增加任意网络或文件访问旁路；现有图片解码与公开能力不能满足时，以通用接口补齐并验证。

遵守插件根部 AI 执行约定。所需接口、权限、宿主接入、插件行为、SDK 与可观察回归在本单闭环；必要预重构先保持行为并验证，不能以占位 UI 或仅协议声明完成工单。保留其他任务改动，新增代码同步补注释。

## Verification

沿真实插件包、公开管理器、原生编辑器交互主接缝验证以上验收项。先运行针对性回归，再执行仓库要求的格式、非 UI workspace 测试与 workspace 编译；涉及 editor-app 追加相关测试和原生验收，实际 WASM 测试先构建再显式运行。记录失败归因与未验证内容，不以 mocked 内部结果替代完整行为。
