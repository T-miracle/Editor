# JavaScript 插件

版本 0.3.0 使用协议 7，为 `.js`、`.mjs`、`.cjs` 和 `.jsx` 提供独立的语言识别、Tree-sitter 高亮、文件图标和默认标准格式化。安装、停用、替换及卸载后已打开文件会重新选择可用提供者。

`native/server.cjs` 使用 TypeScript 5.8.3 的标准 JavaScript/JSX 格式化器，清单声明 `primary=false` 与独立的 `language.formatting` 能力。用户或项目可选择另一个插件提供的格式化器，选择不会更换语法高亮、语言识别或主分析服务。手动格式化只调用所选提供者一次；保存时格式化默认关闭，可按语言开启。

首次安装需要批准 `process.service.format` 与 `dependencies.prepare`。宿主按清单固定版本与 SHA-256，将 Node 24.19.0 和包内脚本准备到插件私有版本目录，通过可执行路径与参数数组启动；不依赖全局 Node 或项目 node_modules。此版本的原生服务支持 Windows x86_64。

本包不需要生命周期 WASM 组件；`grammar/javascript.wasm` 是语法资源。执行 `editor-app.exe --plugin-package plugins/javascript`，共享描述 `nanobug-plugin.json` 将已构建服务、许可证（含 TypeScript 第三方 Unicode 数据通知）、清单、查询、图标与 grammar 封装为项目根部的 `javascript-0.3.0.zip`。使用 `--output` 或宿主“插件打包”配置选择其他输出目录。服务源码变动时，先按[原生语言服务资源](../../installer/README.md#原生语言服务资源)直接重建服务与摘要；不调用归档脚本。
