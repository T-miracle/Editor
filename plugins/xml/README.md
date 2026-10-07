# XML 插件 / XML plugin

版本 0.1.0，为 `.xml`、`.svg`、`.xsd`、`.xsl`、`.xslt` 提供动态 Tree-sitter XML
高亮及 LemMinX 补全、语法/Schema 诊断、悬浮说明和定义跳转。文件扩展名关联通过宿主
通用语言设置配置。SVG 预览由独立 Image 插件提供，两者没有依赖或相互调用。

Windows x86_64 安装授权后下载固定版本 LemMinX 0.31.2 原生程序，来源为
[vscode-xml 0.29.3](https://github.com/redhat-developer/vscode-xml/releases/tag/0.29.3)，
ZIP SHA-256 为 `7eaefaac68253b0ec8e0ad1f1c0f2d0755423d4e99e52497428b52f80df28eb7`。
原生程序使用 EPL-2.0，免装 Java，不修改全局 PATH。
其他平台可显式配置本机 LemMinX 可执行文件；本次自动依赖分发只覆盖 Windows x86_64。

首次安装服务准备失败仍保留高亮，语言服务错误显示在宿主中；更新准备失败保留旧版本。
`executable` 显式指定绝对可执行路径时不下载自动服务，错误路径不会静默回退。

插件设置允许用户或项目覆盖：

- `download_schemas` 默认 `false`，只使用本地、随包及已有缓存的规则；开启后由服务按需下载并缓存。
- `schema_associations` 是 JSON 数组，例如 `[{"pattern":"**/settings.xml","systemId":"schemas/settings.xsd"}]`。
  相对路径以工作区为根，可引用 XSD 或 DTD；显式加载失败会产生配置/加载诊断。
- `catalogs` 是 JSON 路径数组，例如 `["schemas/catalog.xml"]`，用于离线解析外部标识符。
- `svg_suggestions` 默认开启，使用随包的宽松 SVG 常用元素/属性辅助 Schema。
  其范围是常用源码编辑提示，不宣称实现完整 SVG 标准约束；用户 Schema 关联优先。

无 Schema 的普通 XML 不显示缺少 Schema 的告警；基础标签/属性提示及语法诊断继续可用。
历史属性建议只依据当前文档；有效 Schema/DTD 的约束优先。当前文档明确报告规则加载失败
或禁止下载时恢复这些基础建议；其他文件的关联或未被使用的 catalog 不影响此文档。
本地规则路径支持包含 `#`、`%` 的目录和文件名；显式 URI 则应按标准 URI 编码书写。
缓存放在实例私有目录，不写入项目。服务以本机用户权限运行，WASM 沙箱不能限制原生程序。
XSD/DTD 可提供合法子元素、属性、枚举值与文档说明；没有定义时不伪造跳转目标。

Build with the host's public `--plugin-cargo` SDK and package using
`scripts/build-plugins.ps1 -Packages xml -HostExe <editor-app.exe>`.
The ZIP includes the guest, grammar, queries, icons and bundled SVG assistance schema.
