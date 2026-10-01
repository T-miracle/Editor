# JavaScript 插件

为 `.js`、`.mjs`、`.cjs` 和 `.jsx` 文件提供 Tree-sitter WASM 语法解析、高亮、语法错误诊断及深浅色文件图标。支持 JavaScript 模块、异步函数、可选链、模板字符串及 JSX。

此插件为声明式资源包，无需 Node.js，不启动独立的 WASM 组件或语言服务器。语法诊断由编辑器基于解析树生成；类型检查、定义跳转和语义补全不包含在本插件中。

运行 `./scripts/build-plugins.ps1` 生成 `dist/plugins/javascript.zip`，在编辑器“插件管理”中选择“安装 / 更新本机插件包”即可安装。启用、停用、更新与卸载使用宿主现有的插件生命周期。

语法来源、固定版本、校验值和许可见 [grammar/README.md](grammar/README.md)。高亮查询合并自同版本上游的 JavaScript、JSX 和参数查询，许可见 `grammar/LICENSE.tree-sitter-javascript`。
