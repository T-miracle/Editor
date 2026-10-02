# 插件服务 `plugin.services` 1.0

消费者声明契约 ID 和 SemVer 范围，不引用某个提供者插件。协议 7 的 WASM 包使用
`plugin_services.provides` 发布契约，使用 `plugin_services.requires` 声明依赖；二者均需
必需能力 `plugin.services: ^1`。调用方另需安装时批准 `services.call`。
这与启动原生程序的 `process` 服务声明相互独立。

每个契约包含具名方法，每个方法显式声明 `parameters`、`result` 和 `permissions`。
Schema 支持 null、boolean、有上下界的 integer、有字节上限的 string、有数量上限的
array 和闭合 record。record 的 `optional` 仅列出可省略字段，未知字段拒绝。
提供者版本必须匹配依赖范围，消费者要求的方法结构与权限必须完全相同；提供者可增加其他方法。

例如消费者声明：

```json
{
  "plugin_services": {
    "requires": {
      "example.echo": {
        "version": "^1",
        "optional": false,
        "methods": {
          "echo": {
            "parameters": {"type": "string", "max_bytes": 2048},
            "result": {"type": "string", "max_bytes": 4096},
            "permissions": []
          }
        }
      }
    }
  }
}
```

提供者把同一方法放到 `provides.example.echo.methods`，并使用具体版本，例如 `1.0.0`。
必需依赖在激活前检查，缺失或多提供者尚未选择时拒绝激活；可选依赖允许启用后处理不可用结果。
宿主启动按依赖就绪情况重试；无进展的缺失或循环依赖结束重试并显示原因。

## 发现、选择和请求

同一逻辑作用域内只有一个兼容提供者时自动选用；多个提供者必须在宿主插件设置中选择。
用户默认选择与明确项目覆盖保存在宿主管理的配置中。应用级服务与工作区级服务分别选择，
不能隐式跨工作区或借用当前活动窗口；项目文件和插件请求都不能替用户选择提供者。

消费者调用 `service::guest::open(contract)` 获取不可持久化的引用，再用
`service::guest::Task::start(&reference, method, arguments, timeout_ms)` 发起请求。
接受请求返回任务句柄；最终结果通过 `Notification::Service(Request { handle, update })`
投递，`Task::update` 负责关联并过滤旧结果。`Task::cancel` 复用公共取消接口。
引用或任务不再需要时使用 `api::guest::close_resource` 释放。

提供者接收 `Notification::Service(Invoke(Invocation))`，在当前有预算的 WASM 回调中
通过 `Output.service_reply` 返回 `Result<serde_json::Value, Failure>`。
宿主校验参数、结果、请求归属和版本；只传输有界值，不传共享内存或宿主内部对象。
提供者切换、退出或重建都会使旧引用失效；切回原提供者也不会复活旧引用。

请求采用公共 Accepted/Progress/Completed/Cancelled 状态。截止时间为 1–300000 ms。
进入提供者回调之前取消报告 `NotExecuted`；执行已开始后只能报告 `WaitingStopped`，
不承诺回滚。WASM 回调还受宿主既有燃料和内存预算约束；排队超时的请求不会执行。
每实例最多 32 个未完成服务请求，宿主队列最多 256 个，每次轮询最多路由 32 个。
参数及结果各最多 64 KiB；声明最多 256 KiB，最多 32 个契约和每契约 32 个方法；
schema 深度最多 8，record 最多 32 字段，array 最多 1024 项。

## 来源权限与生命周期

`Invocation.caller` 的插件、实例、作用域与权限由宿主生成。消费者和提供者必须同时拥有
方法列出的权限；每一跳只能继续缩小权限。1.0 支持委托 workspace.read、editor.read、
editor.write、ui.panels 和 process.exec；没有声明的委托权限一律拒绝。
提供者可读取自己的不可变包资源，但不能借服务调用读取其私有数据或操作原先创建的私有进程。

异步编辑器请求、进程事件及嵌套服务结果继承原始来源和收缩后的授权；不会在回调时
恢复提供者完整权限。本次委托创建的资源允许显式释放或取消。来源任一实例退出后，
宿主封闭其未完成操作、回收委托资源，并停止投递迟到回调；已发生的外部副作用不视为回滚。
链中重复实例立即拒绝，最多 8 个实例；嵌套调用进入下一轮队列，不递归进入 WASM store。

独立验证包使用 `capability-example` 的公开 SDK 构建产物，由测试打包成消费者及两个
不同 ID 的提供者；宿主运行时没有任何示例插件 ID 或方法名分支。
