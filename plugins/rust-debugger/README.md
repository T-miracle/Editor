# Rust Debugger

独立 WASM 调试提供者，使用公开 `debug.session` 1.1、`plugin.services` 1.1 和 `process` 1.5。安装后对新启动的 Rust/MSVC 或 GNU 调试产物设置源码断点、暂停、继续、步入、步过、步出，并读取调用栈与栈帧局部变量。只支持启动受管目标，不提供附加入口。

CodeLLDB 1.12.3 固定下载和校验，原生桥接程序随包分发；服务依赖放入插件私有版本目录，不修改全局环境。安装需要确认执行、面板与依赖权限，不会安装 Rust 编译器。准备阶段不启动调试器。

通过实际宿主 `--plugin-cargo` 公开入口独立构建组件，再用 rustc 编译原生桥接，将其实际 SHA-256 写入分发清单并直接 ZIP 归档，步骤见[直接打包说明](../../installer/README.md)。源清单中的全零桥接摘要是构建占位，不能直接安装。DAP stdout 与诊断 stderr 分离，每个目标拥有独立的适配器进程树。

取消等待只撤销该回复，不会声称已回滚目标；停止与来源撤销释放整个受管进程树。暂停引用在继续、步进及新停止事件后失效，迟到回复不会复用旧暂停数据。

Official adapter: [CodeLLDB 1.12.3](https://github.com/vadimcn/codelldb/releases/tag/v1.12.3), [license](https://github.com/vadimcn/codelldb/blob/v1.12.3/LICENSE).
