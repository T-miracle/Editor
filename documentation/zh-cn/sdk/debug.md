# 调试会话 — debug.session 1.1

[English](../../en/sdk/debug.md)

调试提供者通过[插件服务](services.md)发布标准 `debug::contract()`。宿主按当前作用域选择兼容提供者；调试适配器和传输由提供者实现，宿主不按插件、语言或适配器名称选择。

## 方法与权限

所有方法要求 process.exec，start 额外要求 ui.panels。

| 方法 | 参数 | 结果 |
| --- | --- | --- |
| start | program、args；可选 cwd、name、env、breakpoints、stop_on_entry | session、state、pause |
| set_breakpoints | session、breakpoints | 断点数组，含 source、line、verified |
| resume | session、pause | session、state、pause |
| pause | session | session、state、pause |
| status | session | session、state、pause；可选 reason、source、line |
| stop | session | session、state、pause |
| step（可选） | session、pause、kind（into/over/out） | session、state、pause |
| frames（可选） | session、pause | 帧数组，含 id、name、source、line |
| variables（可选） | session、pause、frame | 变量数组，含 name、value |

启动使用字面可执行程序与参数数组，沿用[运行会话](sessions.md)的 128 × 4096 字节参数及 64 项环境限额。会话 ID 最多 128 UTF-8 字节。状态为 connecting、running、paused、exited、failed；创建或控制受理不证明目标已暂停或清理完成。

resume、pause、status、stop、设置断点为必需方法。单步可选；只有同时声明 frames 与 variables 才提供检查。缺少可选方法只让相应控件带原因禁用。

## 暂停代次与检查

1.1 在状态、启动与控制结果中加入必需的非负 pause 代次，resume、step、frames、variables 参数也必须携带它，范围不超过 64 位有符号整数。每次观察到新的暂停都推进代次；操作必须使用当前确切的暂停会话与代次。继续、停止或再次暂停使旧帧、变量、选中帧和待完成检查失效；旧请求明确失败，不能继续或单步另一次暂停。被拒绝的控制保留当前暂停检查。

提供者报告真实目标状态和检查；程序打印文字不等于调用栈或变量。选择帧后按同一代次读取该帧变量。帧 ID 为 0–2147483647，最多返回 256 帧，每帧最多 512 变量；名称 512 字节、值 4096 字节，整个服务回复仍限制在 64 KiB。过大结果必须失败，不能将部分检查伪装完整。

## 断点与展示

最多接受 512 个断点，源路径 4096 字节，行号 1–2147483647。每个断点报告 verified；false 或缺省验证不代表绑定成功。绑定回执归属于发出请求的配置／会话，即使用户等待期间选中了其他会话。

宿主原生面板显示会话选择、断点验证、当前位置、控制、帧与变量。按真实源位置打开编辑器，每次新暂停跟随一次。隐藏面板保留会话；选择提供者只影响未来启动，不重路由旧控制。宿主不把时间、程序输出或猜测位置当作暂停。

## 清理与恢复

提供者拥有目标、适配器及委派原生资源。停止调试请求提供者清理；宿主有界等待，只能强制结束该会话所属进程树。提供者停用、卸载、替换、失败及工作区／窗口退役撤销对应资源根和检查，不影响无关会话。迟到回复不能复活旧会话或重放用户命令。

更新采用事务准备：准备失败保留旧实例；激活失败恢复逻辑状态及受管私有数据。已结束的进程内存和外部副作用不能恢复；切回或故障恢复不静默启动目标。

1.0 提供者须将声明和暂停校验更新至 1.1；只有方法名匹配而参数、结果与权限结构不精确兼容，不算可用。
