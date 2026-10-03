# 08 — 从预览导航标题、文档和网页

**Status:** ready-for-agent — 用户已要求执行，按阻塞关系实施。

**GitHub:** [#34](https://github.com/T-miracle/Editor/issues/34)；父方案 [#26](https://github.com/T-miracle/Editor/issues/26)。

**规格：** [Markdown 插件总方案](../spec.md)。验收覆盖：M13、M06、M15、M16。

## What to build

用户点击预览链接，分别跳转当前文档标题、在编辑器打开相对 Markdown 文档或调用系统浏览器。

## Acceptance criteria

- [ ] 标题锚点定位对应渲染块，并对重复标题与无效锚点制定可测试的稳定规则。
- [ ] 相对 Markdown 链接以源文档位置解析，经现有受控文档打开入口处理，不由插件私自创建编辑器状态。
- [ ] 网页链接只在用户点击时交给系统浏览器入口，解析和布局不自动触发外部程序。
- [ ] 过期事件不在错误文档上执行导航；未知协议不转成 Shell 命令。
- [ ] 测试通过既有外部程序边界记录浏览器请求，并通过原生交互验证文档与锚点导航。

## Blocked by

- 02 — 编辑未保存 Markdown 并实时查看原生分栏预览

## Implementation guardrails

非 Markdown 文件与其他 URL 协议沿用现有产品行为，不新增任意执行能力。

遵守插件根部 AI 执行约定。所需接口、权限、宿主接入、插件行为、SDK 与可观察回归在本单闭环；必要预重构先保持行为并验证，不能以占位 UI 或仅协议声明完成工单。保留其他任务改动，新增代码同步补注释。

## Verification

沿真实插件包、公开管理器、原生编辑器交互主接缝验证以上验收项。先运行针对性回归，再执行仓库要求的格式、非 UI workspace 测试与 workspace 编译；涉及 editor-app 追加相关测试和原生验收，实际 WASM 测试先构建再显式运行。记录失败归因与未验证内容，不以 mocked 内部结果替代完整行为。
