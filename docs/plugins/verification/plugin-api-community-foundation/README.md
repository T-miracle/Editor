# 基础社区生态：实施与验收入口

日期：2026-10-09。状态：实施中，未完成的验收项和工单保持开放。

依据：[已批准方案](../../specs/plugin-api-community-foundation.md)、[七张工单及测试责任](../../tickets/plugin-api-community-foundation/README.md)、[实际议题映射](../../tickets/plugin-api-community-foundation/publication.json)。父设计议题 [#92](https://github.com/T-miracle/Nanobug/issues/92) 不随实施工单关闭。

## 工作树与审查基线

- 集成工作树：`C:/Users/Tmiracle/.codex/worktrees/plugin-api-community/Editor`，分支 `codex/plugin-api-community`。
- 批次固定基线：`452994e6d3d6da526803cc62db96ee722580e670`，只包含已批准方案、工单和发布映射。
- 02 工作树：`C:/Users/Tmiracle/.codex/worktrees/plugin-api-interaction/Editor`，分支 `codex/plugin-api-interaction`，从相同基线开始。
- 06 工作树：`C:/Users/Tmiracle/.codex/worktrees/plugin-api-cloud/Editor`，分支 `codex/plugin-api-cloud`，从已交付 02 的 `8506cc8f1f9baee3fca030d59b7d1d21b5227763` 开始。
- 原项目工作区已有其他任务的未提交改动，本批代码在独立工作树实施；这些已有改动不计入本批成果。

## 当前进度

| 工单 | 议题 | 当前状态 | 交付条件 |
| --- | --- | --- | --- |
| 01 文档与只读资源 | [#93](https://github.com/T-miracle/Nanobug/issues/93) | 已验收交付，议题已关闭 | `7b50045a78dacafacb2bb7a76f0694c91a1b755d` 已推送并核对远端；双轴未解决项均为 0 |
| 02 命令与交互 | [#94](https://github.com/T-miracle/Nanobug/issues/94) | 已验收交付，议题已关闭 | `8506cc8f1f9baee3fca030d59b7d1d21b5227763` 已推送并核对远端；双轴未解决项均为 0 |
| 03 工作区事务 | [#95](https://github.com/T-miracle/Nanobug/issues/95) | 依赖已交付，待整合后实施 | T11–T16 及共同完成条件 |
| 04 诊断与语言 | [#96](https://github.com/T-miracle/Nanobug/issues/96) | 等待 03 | T17–T20 及共同完成条件 |
| 05 树与装饰 | [#97](https://github.com/T-miracle/Nanobug/issues/97) | 依赖已交付，待整合后实施 | T21–T24 及共同完成条件 |
| 06 网络与私有状态 | [#98](https://github.com/T-miracle/Nanobug/issues/98) | 实施中，02 依赖已满足 | T25–T30 及共同完成条件 |
| 07 激活与兼容 | [#99](https://github.com/T-miracle/Nanobug/issues/99) | 等待 04、05、06 | T31–T35 及共同完成条件 |

## 证据与复用规则

01 的[逐单验收与双轴结论](01-documents-and-virtual-resources.md)及 [01 主代理原生验收](native-01.md)已经交付；2026-10-09 07:22:13 UTC，按既有 Git 认证更新并读回 #93 为 `closed / completed`。初轮失败保留原始结果，修复候选分别复核；父设计议题 #92 未修改。02 的原生证据见 [02 主代理原生验收](native-02.md)。

02 的[逐单验收与双轴结论](https://github.com/T-miracle/Nanobug/blob/8506cc8f1f9baee3fca030d59b7d1d21b5227763/docs/plugins/verification/plugin-api-community-foundation/02-commands-and-interaction.md)已经发布到独立分支，随后与 01 集成。2026-10-09 06:01:59 UTC，GitHub connector 更新返回 403；按既有工单授权改用既有 Git 认证直接更新，随即读回 #94 为 `closed / completed`。没有输出凭据或修改父设计议题。

每单记录自己的实际源码版本、工具链、SDK、包版本与 hash、测试命令和结果、原生操作、审查结论及未验证部分。协议准入或某个子用例通过不等于整项验收通过，默认跳过的 ignored 测试也不计为通过。

详细用例依照工单目录的唯一主责分配；后单只增加连接测试，重跑实际修改所影响的既有用例。相同源码、SDK、清单、资源、工具链及参数的包可复用产物，每次运行使用独立数据目录。每单的三项仓库交付检查以及相关应用、原生和实际 WASM 验收仍须执行。

审查分别记录 Standards 和 Spec 两条轴，并使用固定提交范围。只有完整验收和审查通过、提交已推送并核对远端后，才更新对应实施议题为已关闭。最终组合验收归 07，不替代前六单的失败行为测试。
