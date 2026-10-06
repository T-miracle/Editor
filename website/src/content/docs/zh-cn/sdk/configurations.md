---
title: 配置模板
description: 插件提供的原生表单、本机运行配置与校验契约。
section: sdk
order: 12
alternate: /en/sdk/configurations/
---

# 配置模板 — run.configurations 1.0

兼容插件通过 `configurations::declaration()` 提供命令模板与自己的原生表单。清单 services 发布该精确契约，协商 `plugin.services >=1.1,<2`，声明 `ui.native ^1` 及表单使用的原生能力。服务路由认证提供者身份，返回数据不能冒充其他提供者。宿主管理配置身份、目录树、本机存储和请求生命周期；插件管理程序策略、默认值、布局和业务校验。

## 方法与返回数据

所有调用含 `workspace`、`locale`、`os` 字符串，分别最多 4096、64、64 字节。返回记录包含最多 64 KiB 的 JSON `payload` 字符串，解码为公开 Rust 类型。`catalog` 和 `validate` 要求 `workspace.read` 与 `process.exec`，用于通过受权限约束的宿主接口解析可用工具；`form` 要求 `workspace.read` 与 `ui.panels`。清单声明不能授予权限，受限工作区不能调用这些服务或执行配置。

`catalog` 返回 `Catalog { templates }`。每个提供者及合并目录最多 128 个模板。模板包含 `id`、`group`、`label`、`icon`、`defaults` 与可选 `unavailable`。身份、分组和名称最多 256 字节；身份和名称不能为空，同一提供者的身份不能重复。默认值是插件自定义 JSON，最多 16 KiB。图标支持 `code`、`terminal`、`play`、`build`、`debug` 或最多 16 KiB 的自包含 SVG，拒绝外部资源与可执行外来内容。普通不可用命令保留并说明原因；环境特定命令（如不可用的 Shell）可由插件省略。选择模板只创建独立草稿，不执行程序或写盘。

`form` 还接收 `template`、`values`、`event`，分别最多 256 字节、16 KiB、64 KiB。空事件初始化表单；其他事件是序列化的原生 `ui::UiEvent`。返回 `Form { values, name, program, document }`，包含插件规范化的 JSON 值与完整原生 `ui::Document`。名称最多 256 字节；程序非空且最多 4096 字节。插件使用原生节点显示名称、只读程序和可编辑的完整参数，可增加字段或选择不同布局；宿主不通过字段身份推测工具业务值。配置表单不能声明编辑器文档源、工具栏、视口或图像输入能力。

每条配置按顺序处理事件。普通回显保留稳定的节点身份和输入框 `value_revision`；仅主动重置或替换输入值时增加该修订，使原生输入、焦点和 IME 组合在刷新时保留。文档修订遵循公开[原生 UI 契约](/zh-cn/sdk/ui/)。

`validate` 接收 `template`、`values` 和最多 64 字节的 `intent`（`save`、`run`、`build`、`debug`），返回 `Validation { valid, message, launch }`。插件可为相同可编辑命令提供构建动作或可调试产物，宿主无需工具专属分支。有效结果必须提供结构化启动数据；无效结果说明可处理的原因且不提供启动数据。服务失败、格式错误、超时或提供者退出都表示无法校验，不能复用旧启动授权。保存等待校验，可以保留无效配置供后续编辑；每次执行都要求同一活跃工作区和提供者实例对未变化值返回新结果。停止既有会话的归属独立保留。

目录、表单和校验请求 30 秒超时；最多同时处理四个目录及 128 个表单／校验请求。原生文档还要通过能力协商、绘制与事件预算。提供者替换或禁用、表单关闭或工作区退出会取消所属请求；迟到结果不能改变替换草稿或授权执行。

## 结构化启动

`Launch` 包含 `target`、可选 `directory`、`env`、`tool_paths`、顺序执行的 `build` 与 `prelaunch` 动作，以及可选执行 `provider`。程序目标为 `{ "mode": "program", "program": "cargo", "args": ["run", "--release"] }`。子命令也是可编辑的完整参数数组的一部分；空格、引号、中文和元字符保留在原来的参数元素中，宿主不把它们拼接为 Shell 命令。

脚本目标为 `{ "mode": "script", "interpreter": "...", "args": [], "script": "..." }`，解释器由插件提供。提供者目标为 `{ "mode": "provided", "provider": "...", "binding": "...", "label": "...", "args": [] }`，通过[运行目标](/zh-cn/sdk/targets/)准备。准备动作为 `{ "name": "...", "target": { "kind": "action", "target": PROGRAM_TARGET } }`；明确引用构建配置时改用 `{ "kind": "build", "config": "CONFIGURATION_ID" }`。这些值仍通过已有配置、路径、工具与会话边界检查，不能授予权限。

仅构建只执行构建动作，不启动最终目标。运行和调试通过公开[会话](/zh-cn/sdk/sessions/)与[调试](/zh-cn/sdk/debug/)契约消费相同的已校验投影。宿主不得为某个包、语言、程序或模板增加专属分支。

配置及目录树按工作区在本机保存。宿主原样保留插件 JSON，不迁移其中字段。插件安装、私有数据、权限和运行进程内存与配置存储各自独立。
