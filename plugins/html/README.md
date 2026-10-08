# HTML 插件

版本 0.3.0 使用协议 7，为 `.html` 和 `.htm` 提供独立的语言识别、Tree-sitter 高亮、文件图标与标准语言服务。语法、分析和格式化分别选择提供者；安装、停用、替换及卸载后已打开文件会重新选择可用提供者。

`native/server.cjs` 使用 vscode-html-languageservice 5.5.0 的 HTML 解析器，提供补全、悬浮提示、文档格式化、F2 重命名与首尾标签联动。解析器处理嵌套标签、空元素、注释和属性；联动默认开启，可按语言关闭。保存时格式化默认关闭，手动格式化使用当前唯一选中的格式化提供者。

标签联动使用公开 `language.editing >=1.1, <2` 的语义配对协商，允许 `<DIV></div>` 等合法的首尾大小写差异；编辑任一端时，用户输入的完整新名称同步到对应端，一次撤销恢复原始两端。服务的标准 LSP 关联编辑仍只返回初始文字完全相同的范围。

首次安装需要批准 `process.service.analysis` 与 `dependencies.prepare`。宿主按清单固定版本与 SHA-256，将 Node 24.19.0 和包内服务脚本准备到插件私有版本目录，通过可执行路径与参数数组启动；不依赖全局 Node 或项目 node_modules。此版本的原生服务支持 Windows x86_64。

本包不需要生命周期 WASM 组件；`grammar/html.wasm` 是语法资源。执行 `editor-app.exe --plugin-package plugins/html`，共享描述 `nanobug-plugin.json` 将已构建服务、许可证、清单、查询、图标与 grammar 封装为项目根部的 `html-0.3.0.zip`。使用 `--output` 或宿主“插件打包”配置选择其他输出目录。服务源码变动时，先按[原生语言服务资源](../../installer/README.md#原生语言服务资源)直接重建服务与摘要；不调用归档脚本。

服务打包使用该固定官方包的 ESM 入口，使解析器静态进入单文件分发；不使用其默认 UMD factory。`@vscode/l10n 0.0.18` 的 npm 包没有许可证文件，随包保留其[固定源码版本的 MIT notice](https://github.com/microsoft/vscode-l10n/blob/fc7e3d79ddb91a2cc24a9730aad912026a43dbf2/LICENSE)。
