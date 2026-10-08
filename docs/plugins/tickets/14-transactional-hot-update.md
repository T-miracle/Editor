# 14 — 热切换含服务与界面的插件并在失败时恢复

**Status:** completed — 已完成实现、验证、独立双轴审查、提交与推送；GitHub #15 已关闭。完成状态于 2026-10-03 同步，见[本工单验收](../verification/plugin-api-hot-update-verification.md)及[最终契约验收](../verification/plugin-api-contract-verification.md)。

**GitHub:** [#15](https://github.com/T-miracle/Editor/issues/15)

**Parent:** [设计基线 #1](https://github.com/T-miracle/Editor/issues/1)

## What to build

一个同时拥有私有状态、服务和界面的插件可以在编辑器运行时更新；新版本激活失败时，旧版本恢复，已打开文档重新连接。

把前序垂直路径汇合到现有管理器的两阶段更新事务，不重复实现独立更新系统。集中处理实例代际和贡献发布边界。

## Acceptance criteria

- [x] 准备阶段旧版本服务与界面继续工作，新版本不抢占贡献、不写正式数据。
- [x] 切换暂停旧新请求，获取最终状态、停止旧服务，替换贡献并激活新版本；允许明确的短暂中断。
- [x] 激活成功后提交记录与数据并清理旧资源；准备、激活及提交失败分别按阶段恢复。
- [x] 语言、面板、插件间服务引用和订阅跟随实例代际变化，迟到事件不复活旧提供者。
- [x] 已打开文档在恢复或成功更新后重新同步；配置及启用作用域保留。
- [x] 故障状态同时保留更新与恢复错误；用户文档不丢失，不保证原生进程 PID 或外部副作用回滚。

## Blocked by

- [#8](https://github.com/T-miracle/Editor/issues/8) — 07：用可选 WASM 钩子启动未知语言的 LSP
- [#11](https://github.com/T-miracle/Editor/issues/11) — 10：在插件面板内组合原生组件与自定义画布
- [#12](https://github.com/T-miracle/Editor/issues/12) — 11：通过版本化服务契约替换插件协作提供者
- [#14](https://github.com/T-miracle/Editor/issues/14) — 13：通过隔离数据副本迁移插件私有状态

## Verification

独立组合夹具在各事务边界注入失败，通过管理器和 GPUI 观察服务、界面、请求、数据及资源变化；沿用现有更新 smoke 驱动。

规格验收覆盖：T08、T09、T10、T11、T22。

## Implementation guardrails

- 使用已确认的现有包管理器与编辑器集成测试边界，不新增插件专属测试入口。
- 只改变本工单范围，保留工作区已有改动；新增代码同时补充意图和不变量注释。
- 复用既有模块；必要的预重构先保持行为不变并通过相关回归，再加入新行为。
- 测试使用隔离目录与夹具，不操作用户真实插件安装数据；不提交 Git、不发布中间版本。
- 流程与契约细化若改变已确认产品行为，应先评审；不能以实现方便引入插件专属分支。
