# 16 — 迁移现有语言包并移除 Rust 专属宿主配置

**Status:** completed — 已完成实现、双轴审查与实际组件回归，详见[验证记录](../plugin-api-language-migration-verification.md)

**GitHub:** [#17](https://github.com/T-miracle/Editor/issues/17)

**Parent:** [设计基线 #1](https://github.com/T-miracle/Editor/issues/1)

## What to build

Rust、TOML、HTML 和 JavaScript 通过新声明与钩子工作；Rust 的语言服务配置和插件开发支持由 Rust 插件实现。

这是广域迁移中的语言批次。复用已经通过陌生语言夹具的通用能力，不修改通用宿主去识别本批插件。

## Acceptance criteria

- [x] 四类语言包迁移声明、grammar、图标及适用服务定义，版本和打包产物同步更新。
- [x] 按需为 Rust 提供 WASM 钩子，迁移 rust-analyzer 初始化、配置节及项目发现逻辑到插件。
- [x] 宿主 SDK 缓存仍为公开能力，Rust 插件通过它维持独立插件项目的补全、悬浮和跳转体验。
- [x] 无 LSP 的资源包保持声明式，不人为增加动态组件或服务依赖。
- [x] 运行时安装、作用域切换、提供者变更及卸载正确影响已打开文件与语法/LSP 结果。
- [x] 删除这批迁移完成的固定语言枚举和 Rust 宿主分支，保留其行为回归，不能仅改名掩盖专属逻辑。

## Blocked by

- [#9](https://github.com/T-miracle/Editor/issues/9) — 08：安装语言插件时自动准备私有服务依赖
- [#16](https://github.com/T-miracle/Editor/issues/16) — 15：升级已安装记录并通过新 SDK 重装插件

## Verification

通过新 SDK 与打包入口生成实际包，验证已打开文档、别名扩展、图标、Rust 初始化和公开 SDK 分析；同时重跑陌生语言夹具。

规格验收覆盖：T01、T02、T03、T04、T05、T11、T23、T25、T26。

## Implementation guardrails

- 使用已确认的现有包管理器与编辑器集成测试边界，不新增插件专属测试入口。
- 只改变本工单范围，保留工作区已有改动；新增代码同时补充意图和不变量注释。
- 复用既有模块；必要的预重构先保持行为不变并通过相关回归，再加入新行为。
- 测试使用隔离目录与夹具，不操作用户真实插件安装数据；不提交 Git、不发布中间版本。
- 流程与契约细化若改变已确认产品行为，应先评审；不能以实现方便引入插件专属分支。
