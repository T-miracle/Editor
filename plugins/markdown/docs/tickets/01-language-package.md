# 01 — 安装插件后识别并高亮 Markdown

**Status:** completed — 实现、真实包验证及双轴审查通过；见[验收记录](../verification/01-language-package.md)。

**GitHub:** [#27](https://github.com/T-miracle/Editor/issues/27)；父方案 [#26](https://github.com/T-miracle/Editor/issues/26)。

**规格：** [Markdown 插件总方案](../spec.md)。验收覆盖：M01（安装与信任）、M02（源码高亮）、M14、M16。

## What to build

通过独立可构建插件包，让已打开的 Markdown 文档在安装启用后获得语言识别与源码高亮，停用后正确降级。

## Acceptance criteria

- [x] 声明 Markdown 文件匹配、Tree-sitter WASM grammar 与查询，资源包无需空组件，可经构建脚本和管理器安装。
- [x] 受信任工作区安装启用后，已有及新打开的 .md／.markdown 文档获得高亮，无须重启；继续受通用工作区信任策略约束。
- [x] 停用、卸载与替换遵守提供者选择，撤销旧 grammar 与贡献，不使用宿主内建回退。
- [x] 新增资源、协议与权限声明通过实际包校验；通过公共管理器验证热生效与资源回收。

## Blocked by

无；拆分获批后可进入实施前沿。

## Implementation guardrails

本单不接入发行默认安装、不提供预览；为后续工单建立真实包纵向验证基础。

遵守插件根部 AI 执行约定。所需接口、权限、宿主接入、插件行为、SDK 与可观察回归在本单闭环；必要预重构先保持行为并验证，不能以占位 UI 或仅协议声明完成工单。保留其他任务改动，新增代码同步补注释。

## Verification

沿真实插件包、公开管理器、原生编辑器交互主接缝验证以上验收项。先运行针对性回归，再执行仓库要求的格式、非 UI workspace 测试与 workspace 编译；涉及 editor-app 追加相关测试和原生验收，实际 WASM 测试先构建再显式运行。记录失败归因与未验证内容，不以 mocked 内部结果替代完整行为。
