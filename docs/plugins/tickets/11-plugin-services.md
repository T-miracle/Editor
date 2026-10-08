# 11 — 通过版本化服务契约替换插件协作提供者

**Status:** completed — 已完成实现、验证、独立双轴审查、提交与推送；GitHub #12 已关闭。完成状态于 2026-10-03 同步，见[本工单验收](../verification/plugin-api-services-verification.md)及[最终契约验收](../verification/plugin-api-contract-verification.md)。

**GitHub:** [#12](https://github.com/T-miracle/Editor/issues/12)

**Parent:** [设计基线 #1](https://github.com/T-miracle/Editor/issues/1)

## What to build

一个消费者插件调用两个可互换提供者之一，切换提供者不修改消费者；缺少服务、提供者退出或越权请求有可见结果。

服务契约注册、发现、选择、请求和撤销形成真实闭环，先用最小协作夹具验证，不依赖终端迁移。

## Acceptance criteria

- [x] 定义服务契约标识、参数、结果和版本匹配，调用者声明依赖服务而非插件 ID。
- [x] 唯一提供者可选用，多个提供者由用户选择，必需与可选服务缺失分别明确拒绝或降级。
- [x] 请求关联、取消与截止时间复用公共调用模型，提供者退出时结清请求并使旧引用失效。
- [x] 调用保留来源和授权范围；不允许消费者隐式借用提供者权限。
- [x] 循环依赖及重入导致的等待有可检测或受限行为，不无限阻塞宿主。
- [x] 不开放内部对象或共享内存；具体插件名只能作为数据和测试样例。

## Blocked by

- [#4](https://github.com/T-miracle/Editor/issues/4) — 03：通过类型化请求操作编辑器并管理异步事件

## Verification

使用消费者与两个不同 ID 的提供者包，通过管理器和选择界面验证替换、取消、退出、版本不匹配和权限传播。

规格验收覆盖：T20。

实现与验收记录见 [服务契约验证](../verification/plugin-api-services-verification.md)。

## Implementation guardrails

- 使用已确认的现有包管理器与编辑器集成测试边界，不新增插件专属测试入口。
- 只改变本工单范围，保留工作区已有改动；新增代码同时补充意图和不变量注释。
- 复用既有模块；必要的预重构先保持行为不变并通过相关回归，再加入新行为。
- 测试使用隔离目录与夹具，不操作用户真实插件安装数据；不提交 Git、不发布中间版本。
- 流程与契约细化若改变已确认产品行为，应先评审；不能以实现方便引入插件专属分支。
