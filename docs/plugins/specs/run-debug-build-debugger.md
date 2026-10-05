# 本批调试器与握手决定（2026-10-05）

适用 [#57](https://github.com/T-miracle/Editor/issues/57)、[#58](https://github.com/T-miracle/Editor/issues/58)；用户已要求完成整批剩余工单及所有缺口。本记录在真实调试提供者实现之前定稿，属于本批范围内的技术选择，不改变原方案只支持启动调试、不提供附加入口的产品边界。

## 调试器和传输

- Windows 首个提供者使用 CodeLLDB 1.12.3。官方说明支持 PDB；保留本机已有 `x86_64-pc-windows-msvc` 工具链，不安装另一套 Rust 编译器或 Windows SDK。[Windows 支持说明](https://github.com/vadimcn/codelldb/wiki/Windows)
- 使用 DAP。DAP 适配、初始化、断点、调用栈和变量映射全部由插件完成，宿主只消费版本化 `debug.session`，不识别 Rust、LLDB 或 DAP 消息。[DAP 官方协议与握手](https://microsoft.github.io/debug-adapter-protocol/overview)
- 已下载并核对官方 Windows x64 包；`codelldb.exe --help` 实测只有 `--port` / `--connect`，没有标准输入输出模式。因此插件自带小型原生传输桥：监听仅本机回环的临时端口，启动已固定版本的 CodeLLDB，用受控标准输入输出转发 DAP 字节。桥、适配器、目标同属宿主创建的受管进程树；不新增宿主网络能力，不监听公共接口。
- 每个调试会话拥有一份桥和适配器。进程退出、连接失败、启动超时、来源撤销和插件实例退休均关闭整棵树。桥不加载进宿主，不提供插件入口；它是插件的受控原生服务依赖。

## 依赖和权限

官方包 URL：`https://github.com/vadimcn/codelldb/releases/download/v1.12.3/codelldb-win32-x64.vsix`。

SHA-256：`a916e509308dac817732f63ca604a8b93ed29cd16f38a2fa9f0b64ed58e8f51a`，与官方 Release API 的 `digest` 一致。验收副本只准备到本工作区 `target/debug-adapters/codelldb-1.12.3/`，没有安装 VS Code 扩展或修改全局环境。[官方发行记录](https://github.com/vadimcn/codelldb/releases/tag/v1.12.3)

交付包通过现有依赖计划准备到宿主私有版本缓存：适配器使用固定 URL/hash；传输桥是包内经过 hash 验证的原生文件。沿现有安装权限与服务授权路径执行，拒绝在受限工作区启动。服务调用仍要求来源 `process.exec`；使用提供者声明的固定服务不能借用提供者私有进程，资源绑定原始调用上下文。

## 只有一个目标的握手

1. 宿主按原配置完成验证、自动保存、构建和启动前步骤，保留本次计划快照。
2. 普通运行把最终程序交给执行提供者；启动调试把同一个最终程序交给调试提供者。调试提供者拥有目标启动权，执行提供者不再另外启动一份程序。
3. 插件启动桥并交换 DAP `initialize`，随后 `launch`；收到 `initialized` 后发送各源码断点与 `configurationDone`，等待真实启动结果。目标的 `program`、`args`、`cwd` 和 `env` 来自本次快照，不通过 Shell 拼接。[CodeLLDB 启动参数](https://github.com/vadimcn/codelldb/blob/master/MANUAL.md)
4. 使用 CodeLLDB 控制台输出方式，不把启动权转交外部终端。输出沿会话身份归属；暂停、继续、单步和检查的结果来自 DAP 响应或事件，不从时间或输出文本猜测。
5. 提供者原实例与目标会话身份贯穿全部控制。暂停后的栈帧与变量引用只在该次暂停内有效；迟到结果不能替换新的暂停或其他配置的面板。

## 异步基础能力

现行服务回调要求在一次 WASM 调用内返回最终结果，无法等待原生适配器响应。本批需要通用的有界延后回复能力：宿主发出不透明调用句柄，提供者可以在原生通知后回复，回复仍检查原声明、来源、期限、实例与作用域。队列有上限；来源退休、请求取消与超时有明确释放路径。原生状态查询和调试请求都等待其真实完成，不能把 500 毫秒轮询窗口当作失败判据。

这是实现所需的通用基础能力；公开协议、SDK、消费者及独立构建验证随实现一并更新。此处记录技术决定，不代表真实断点、步进、原生提示或工单验收已经完成。
