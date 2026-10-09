---
title: 类型化命令与原生菜单
description: 经过校验的命令参数、结果、取消及原生目标贡献。
section: sdk
order: 17
alternate: /en/sdk/commands/
---

# 类型化命令与原生菜单

发现或提供 typed 命令、贡献原生菜单需协商 `plugin.commands ^1`。调用其他插件另需已批准的 `commands.call`。没有 `signature` 的命令保留既有单向 `Notification::Command` 语义，不作为有返回值的命令被发现。

## 注册与调用

清单命令可用 `service::Method` 声明 `parameters`、`result` 和 `permissions`。安装后的调用方和提供者必须同时授权签名，提供者需在清单中声明这些权限。schema 支持 null、布尔、有界整数、有界 UTF-8 字符串、有界数组和封闭记录。未知字段与错误参数在执行前拒绝，错误返回值产生明确失败。

```json
{
  "id": "echo", "title": "Echo",
  "signature": {
    "parameters": { "type": "string", "max_bytes": 64 },
    "result": { "type": "string", "max_bytes": 64 },
    "permissions": []
  },
  "menus": [{ "location": "editor", "group": "tools", "order": 10, "arguments": "hello" }]
}
```

`commands::discover()` 返回调用方作用域内活动的 `Descriptor { plugin, command, signature }`，不执行提供者或授予权限。`commands::invoke(plugin, command, arguments, timeout_ms)` 返回 accepted 请求句柄。`commands::Task` 关联 `Notification::CommandRequest` 更新，并通过普通宿主取消 API 取消。

提供者收到 `Notification::CommandInvocation(Invocation { id, arguments, context, caller, reply })`。宿主认证原始调用方及逐步收缩的权限交集，不借用提供者更强的私有授权。可选原生 `context` 是独立于参数 schema 的描述信息。通过 `Output.service_reply` 立即返回，或保留提供者自己的 `reply` 句柄，之后调用 `commands::reply(reply, result)`。空 output 表示延后工作，不是成功 null。`Notification::CommandCancelled` 让插件清理本地记录；取消、超时、重复和外来回复均拒绝。

嵌套调用保留原始来源，循环和超过八层的调用拒绝。替换、退役或跨工作区实例不能接收旧结果。所选资源授权不能通过命令或服务转授，即使双方均有 `files.select`。

参数与回复最多 64 KiB，每实例最多 32 个待决请求和 32 个延后调用。截止时间为 1–300000 毫秒，最先发生的终态生效。取消停止等待并关闭该等待下的待决原生交互，不回滚已进入的副作用，也不代表终止已经拥有的程序。

## 原生贡献

每命令最多声明 16 项 `menus`。`location` 为 `editor`、`selection`、`explorer` 或 `tab`，选区项需实际选区存在。`group` 最多 64 字节，`order` 为有符号整数。按 group、order、插件 ID、命令 ID 和贡献下标排序。分组之间显示分隔，标签注明来源插件。

`when` 控制可见性，`enabled_when` 控制启用状态。封闭条件可包含 `has_selection`、`writable`、`directory`、`language` 和 `extension`，每个指定字段必须匹配，省略条件为 true。语言标识最多 128 字节，扩展名最多 32 字节。宿主在激活时重新检查条件，并保留原目标及实例 incarnation。

静态 `arguments` 默认 null，必须符合 schema 和 64 KiB 上限。独立原生 `Context` 包含 `has_selection`、`writable`、`directory`、`language`、`extension` 和可选的工作区相对 `path`，描述点击目标，可能不同于活动编辑器。虚拟资源不变成磁盘路径。上下文不授予文件访问，也不替代版本、选择授权或资源句柄。

禁用、卸载、故障、撤销信任和替换移除不可用贡献。旧回调不能针对新实例或变更/关闭的文档执行。菜单与普通命令入口使用同一经过检查的路由。

经确认的输入、消息、进度及外部文件选择见[原生交互与所选资源](/zh-cn/sdk/interaction/)。
