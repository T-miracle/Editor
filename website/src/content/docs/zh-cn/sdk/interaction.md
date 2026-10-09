---
title: 原生交互与所选资源
description: 经确认的原生输入、取消、带来源消息与受限文件选择。
section: sdk
order: 18
alternate: /en/sdk/interaction/
---

# 原生交互

快选、输入、确认、通知和进度需协商 `ui.interaction ^1` 并批准 `ui.interaction`。文件、目录和保存位置选择则需 `files.selection ^1` 与已批准的 `files.select` 权限。这些操作通过 `api::EditorOperation::Interaction` 在活动、可信的工作区实例中运行。

`ui.interaction` 与 `files.selection` **1.0.0 为稳定接口**，不是实验能力。最低宿主需支持当前 `protocol = 7`、`api.base = ^1` 与相应能力的 `^1` 范围。无法协商 required 能力时拒绝包；optional 能力缺失时，禁用相应功能或使用插件自身降级。[公共稳定性、兼容与弃用政策](/zh-cn/sdk/#稳定性与能力兼容)适用于这两项接口。

文件、目录及保存位置的原生选择器当前支持 Windows。其他平台的 `Select` 返回 `UnsupportedOperation`，即使已协商 `files.selection`；插件应将该失败与用户取消分别处理。

`interaction::start(operation, timeout_ms)` 返回所属实例的 accepted 句柄。结果通过 `Notification::Request` 的 `RequestUpdate<EditorValue>` 投递，成功值包装为 `EditorValue::Interaction`。`api::guest::EditorTask` 关联该句柄并忽略其他任务和重复终态。Accepted 只表示排队，并不表示用户已经确认。

## 确认值与消息

| 操作 | 成功值 | 用户动作 |
| --- | --- | --- |
| `QuickPick` | `Value::Picked(id)` | 过滤标签/说明，选择稳定的条目 ID |
| `Input` | `Value::Input(text)` | 确认有 UTF-8 字节上限的字符串；密码模式遮蔽显示 |
| `Confirm` | `Value::Confirmed` | 明确确认显示的消息 |
| `Notify` | `Value::Dismissed` | 关闭带插件来源的通知 |
| `Progress` | `Value::Finished` | 所属插件调用 `interaction::finish(handle)` |
| `Select` | `Value::Selected(resources)` | 确认系统文件、目录或保存选择器 |

Escape、取消按钮、原生对话框关闭、超时和所属实例退役产生明确的取消或失败终态，不返回空成功选择、false 确认或虚构输入。迟到结果不能覆盖终态。取消 typed 命令也会关闭仍在该命令下等待的原生交互，但不承诺撤销已经发生的效果。

输入与快选复用宿主原生输入行为，包括 IME 组合输入。模态提示在仍拥有焦点时恢复此前焦点。通知和进度在宿主消息区域显示插件来源并保留编辑器焦点，不进入宿主持久错误历史。每个窗口同一时刻只显示一个模态交互，已接受的其他请求按顺序等待。

`interaction::update(handle, message, percent)` 替换原进度任务的显示，不分配第二个任务。百分比可省略，范围为 0–100。可取消任务提供用户取消动作。插件继续自身工作前必须消费取消状态，并处理被拒绝的迟到更新或结束操作。

标题为 1–256 个 UTF-8 字节，消息最多 4096 字节。快选包含 1–512 个条目，唯一 ID 为 1–128 字节，标签为 1–256 字节，可选说明最多 1024 字节。输入预算为 1–65536 字节，初值必须符合该预算；占位文字最多 256 字节。每实例最多 32 个待决编辑器请求，截止时间为 1–300000 毫秒。Windows 原生选择器另限制为最多四个工作线程和 64 个原生路径；每批选择最多授予 32 个资源，整批校验成功后才分配句柄。

## 受限选择授权

`Select` 包含标题、`SelectionMode::{File,Directory,Save}`、多选标志和可选建议文件名。Save 模式仅单选。建议名最多 255 字节，不含斜线、反斜线、冒号、NUL，且不为 `.` 或 `..`；建议名本身不产生访问授权。

原生路径只交付宿主。每个返回的 `SelectedResource` 包含 opaque `handle`、描述用 `name` 和 `kind`。选择实例拥有精确目标和允许的操作。句柄是临时资源，不应持久化或转授。

| 所选类型 | 当前可执行操作 |
| --- | --- |
| File | `api::guest::read_file(&handle, "")` 只读该文件 |
| Directory | `read_file(&handle, "relative/path")` 只读所选目录边界内的普通文件 |
| Save | 保留精确目标保存意图；`ReadFile` 拒绝，`WriteFile` 返回 `UnsupportedOperation`，直至安全宿主文件事务消费该意图 |

选择不批准任意工作区或外部写入。目录穿越、绝对/驱动器/设备路径、备用数据流，以及越过选择边界的链接或 junction 均拒绝。闲置授权不锁住文件，不阻碍正常改名或原子替换；替换所选对象使旧句柄失效，不会授予访问新对象的权限。

`api::guest::close_resource(handle)` 按值接收句柄并释放授权。禁用、卸载、撤销信任和实例替换撤销剩余授权。取消、超时和迟到选择回调不创建授权。委托服务与跨插件命令上下文禁止选择和所选资源读取，即使双方分别拥有 `files.select`；通过 JSON 值或返回值也不能转授权限。

宿主直接调用的 typed 命令可以选择、读取和释放自己的资源，前提是其签名要求 `files.select` 且安装时已批准该权限。运行时以内部记录确认宿主来源，只允许目标提供者的第一跳；访客提供的 `@host` 等调用者名称不构成来源证据。嵌套和跨插件调用仍被拒绝，普通服务方法不能把 `files.select` 声明为可委托权限。

可复用入口见[类型化命令与菜单](/zh-cn/sdk/commands/)。菜单路径是描述信息，文件访问仍需普通工作区或选择 API 的授权。
