# HTML 插件

为 `.html`、`.htm` 文件提供 Tree-sitter WASM 解析、语法高亮和深浅色文件图标。高亮覆盖标签、属性名、带引号及不带引号的属性值、注释、DOCTYPE 和字符实体；解析器报告的语法错误由编辑器显示。

此插件是由宿主管理的声明式资源包，无额外权限或独立组件。运行仓库根目录的 `./scripts/build-plugins.ps1` 后，在插件管理中安装 `dist/plugins/html.zip`；启用、停用和卸载会同步更新已打开文件。

本版本仅提供 HTML 语法层支持，不包含 LSP 补全、格式化或 HTML 规范校验；`script`、`style` 内容按原始文本解析，暂不注入 JavaScript/CSS 高亮。解析器允许 HTML 的部分省略标签写法，因此语法诊断不等同于完整的 HTML 校验器。

语法来源、固定版本、校验值和 MIT 许可见 [grammar/README.md](grammar/README.md)。
