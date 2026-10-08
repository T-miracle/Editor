# 08 — 安装语言插件时自动准备私有服务依赖

**Status:** completed — 已完成实现、验证、独立双轴审查、提交与推送；GitHub #9 已关闭。完成状态于 2026-10-03 同步，见[本工单验收](../verification/plugin-api-dependencies-verification.md)及[最终契约验收](../verification/plugin-api-contract-verification.md)。

**GitHub:** [#9](https://github.com/T-miracle/Editor/issues/9)

**Parent:** [设计基线 #1](https://github.com/T-miracle/Editor/issues/1)

## What to build

未预装服务的干净环境中，用户安装语言插件并授权后自动下载、校验和解包服务，随后启动 LSP；下载失败在管理界面可见且可重试。

将通用依赖计划接入实际语言插件安装路径，包含声明来源与可选动态解析。只支持声明式文件准备，不执行安装脚本。

## Acceptance criteria

- [x] 依赖声明或钩子方案明确来源、版本、平台、校验信息及可执行入口，宿主通用执行。
- [x] 服务与必要运行时使用私有版本目录，缓存可复用，不修改系统 PATH 或全局安装。
- [x] 同时支持包内携带和显式本机程序；生效顺序遵守配置契约，离线可用来源能运行。
- [x] 安装界面区分下载、验证、服务启动和就绪；校验失败、取消和网络错误可诊断，不伪报安装可用。
- [x] 缓存回收保护正在使用及更新恢复所需版本，不因一个项目卸载破坏另一项目。
- [x] 新增依赖解析和安装循环检测，失败不影响旧插件准备期间继续运行。

实现与证据：[私有依赖验证](../verification/plugin-api-dependencies-verification.md)。原模板“不提交 Git”已由用户连续执行授权覆盖，见 `docs/agents/issue-tracker.md`。

## Blocked by

- [#8](https://github.com/T-miracle/Editor/issues/8) — 07：用可选 WASM 钩子启动未知语言的 LSP

## Verification

本地下载源提供正确包、损坏包与失败响应；干净临时环境真实安装到 LSP 就绪，验证缓存复用及取消。

规格验收覆盖：T06、T07。

## Implementation guardrails

- 使用已确认的现有包管理器与编辑器集成测试边界，不新增插件专属测试入口。
- 只改变本工单范围，保留工作区已有改动；新增代码同时补充意图和不变量注释。
- 复用既有模块；必要的预重构先保持行为不变并通过相关回归，再加入新行为。
- 测试使用隔离目录与夹具，不操作用户真实插件安装数据；不提交 Git、不发布中间版本。
- 流程与契约细化若改变已确认产品行为，应先评审；不能以实现方便引入插件专属分支。
