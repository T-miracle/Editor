# 10 — 在插件面板内组合原生组件与自定义画布

**Status:** completed — 已完成实现、验证、独立双轴审查、提交与推送；GitHub #11 已关闭。完成状态于 2026-10-03 同步，见[本工单验收](../verification/plugin-api-composable-ui-verification.md)及[最终契约验收](../verification/plugin-api-contract-verification.md)。

**GitHub:** [#11](https://github.com/T-miracle/Editor/issues/11)

**Parent:** [设计基线 #1](https://github.com/T-miracle/Editor/issues/1)

## What to build

一个独立插件用标准工具栏控制自定义画布，并提供原生输入框与编辑器内预览；焦点、输入法、尺寸和主题交互完整可用。

在新协议中建立可组合 UI 路径，保留 gpui-base 行为与编辑器本地外观。能力分版，不将旧终端布局直接提升为所有插件必需接口。

## Acceptance criteria

- [x] 支持纯标准组件、纯画布及同一界面组合，布局中没有依赖具体插件的专属槽位。
- [x] 原生输入法、焦点、键盘和指针事件准确路由，画布不抢占输入框事件。
- [x] 字符网格尺寸为按需能力，普通表单与图像画布不依赖网格字段。
- [x] 主题、字体和尺寸变化热生效，通用图片或向量绘制可用于文件预览。
- [x] 面板与预览贡献按照声明注册撤销，隐藏和卸载回收布局空间及事件目标。
- [x] UI 能力不隐式授予文档读取或进程权限；无效布局和失效节点返回明确错误。

## Blocked by

- [#4](https://github.com/T-miracle/Editor/issues/4) — 03：通过类型化请求操作编辑器并管理异步事件

## Verification

GPUI 驱动标准表单、工具栏加画布和内存文档预览夹具，覆盖 IME、焦点、缩放尺寸、主题及隐藏回收。

规格验收覆盖：T15、T22。

实测记录：[组合 UI 验证](../verification/plugin-api-composable-ui-verification.md)。

## Implementation guardrails

- 使用已确认的现有包管理器与编辑器集成测试边界，不新增插件专属测试入口。
- 只改变本工单范围，保留工作区已有改动；新增代码同时补充意图和不变量注释。
- 复用既有模块；必要的预重构先保持行为不变并通过相关回归，再加入新行为。
- 测试使用隔离目录与夹具，不操作用户真实插件安装数据；不提交 Git、不发布中间版本。
- 流程与契约细化若改变已确认产品行为，应先评审；不能以实现方便引入插件专属分支。
