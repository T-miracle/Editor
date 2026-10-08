# Rust Debugger

独立 WASM 调试提供者，使用公开 `debug.session` 1.1、`plugin.services` 1.1 和 `process` 1.5。安装后对新启动的 Rust/MSVC 或 GNU 调试产物设置源码断点、暂停、继续、步入、步过、步出，并读取调用栈与栈帧局部变量。只支持启动受管目标，不提供附加入口。

CodeLLDB 1.12.3 固定下载和校验，原生桥接程序随包分发；服务依赖放入插件私有版本目录，不修改全局环境。安装需要确认执行、面板与依赖权限，不会安装 Rust 编译器。准备阶段不启动调试器。

通过宿主 `editor-app.exe --plugin-package plugins/rust-debugger` 完整构建并封装 ZIP，默认输出为插件项目根部的 `rust-debugger-0.1.1.zip`。项目描述在 `nanobug-plugin.json`，也可使用宿主“插件打包”配置选择输出位置；不调用归档脚本。 原生桥接的实际 SHA-256 写入分发清单，源清单的占位摘要不变；DAP stdout 与诊断 stderr 分离。

取消等待只撤销该回复，不会声称已回滚目标；停止与来源撤销释放整个受管进程树。暂停引用在继续、步进及新停止事件后失效，迟到回复不会复用旧暂停数据。

Official adapter: [CodeLLDB 1.12.3](https://github.com/vadimcn/codelldb/releases/tag/v1.12.3), [license](https://github.com/vadimcn/codelldb/blob/v1.12.3/LICENSE).
