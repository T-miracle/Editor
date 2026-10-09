# 声明式语言包

[English](../../en/sdk/languages.md)

协议 7 的包可以省略 `component`。它的 `manifest.json` 把 `contributions` 指向包内相对的
TOML 文件，声明 `api.base = "^1"`，且权限、面板与命令均为空。识别与高亮不需要任何可执行的
生命周期来宾。

```json
{"id":"novel-language","name":"Novel language","version":"1.0.0","protocol":7,"api":{"base":"^1"},"contributions":"plugin.toml","storage_limit":1024}
```

贡献元数据必须与包 ID 和版本一致：

```toml
[plugin]
id = "novel-language"
name = "Novel language"
version = "1.0.0"
host_version = ">=0.1.0"

[[language_definitions]]
id = "novel"
name = "Novel"
extensions = ["novel"]
filenames = ["Novelconfig"]

[[highlighters]]
id = "syntax"
language = "novel"
grammar_name = "toml"
grammar = "grammar/toml.wasm"
highlights = "queries/highlights.scm"
tree_sitter_abi = 15
```

这个示例识别一个新的语言身份，同时复用兼容的 TOML 语法。`grammar_name` 是 Tree-sitter 的
模块导出名，与文档语言 ID 无关。请在这些路径上放置真实的 WASM grammar 与查询文件；宿主在
发布前校验它们的 ABI 与查询。校验失败时不会启用任何原生 grammar 回退。

每个数组最多 64 项。语言与高亮 ID 使用小写 ASCII 字母、数字、点、下划线或连字符，最长
100 字节。`text` 保留给不参与解析的纯文本。每种语言必须有非空名称、至少一个选择器，最多
128 个选择器。扩展名不带点，文件名为基名。匹配不区分大小写，精确文件名优先，同一份定义内
等价重复的选择器会被拒绝。schema 目前接受 Tree-sitter ABI 14 或 15，运行时校验还会检查模块
的真实 ABI。所有资源路径都保持在不可变的包目录内。

一个包可以贡献多种语言、只贡献识别、只贡献高亮，或两者兼具。来自不同包的提供者可以组合。
识别选择以文件选择器为键，高亮选择以语言 ID 为键。提供者身份是包 ID 加贡献 ID，与包版本
无关。

高亮提供者可选声明 `injections`（包内相对查询路径）与 `injection_languages`（最多 64 个允许
注入的语言身份）。注入查询在发布前使用同一份已验证的 WASM grammar 编译；每个查询模式必须
静态指定列表内的 `injection.language`，动态语言捕获会被拒绝。各被注入语言使用独立选择的
提供者与解析器工厂。提供者缺失或停用时显示普通文本，不回退原生 grammar。已有包默认不声明
查询、语言列表为空；插件可组合块级与行内 grammar，宿主不承担特定语言规则。

原生 **设置 → 语言提供者** 页面分别为识别与高亮选择用户偏好或已确认的本地项目偏好。项目
选择覆盖用户选择。唯一提供者会被自动采用并记住；出现竞争提供者时仍保留有效选择。被选中的
提供者被移除时，若只剩一个候选则采用它，若仍有多个则询问用户，若一个都不剩则报告纯文本回退。
重置会移除所选偏好层并在重新解析前放弃其记忆的选择。

选择保存在宿主的插件管理目录中，位于来宾私有数据与项目文件之外，且不授予任何权限或工作区
信任。被禁用、失败或不受信任的贡献会被撤回；生命周期变化会刷新已打开的编辑器，但不重新
打开文件。后台加载只准备 grammar 数据。注册前会核对当前任务代际与所选包版本，因此已退役的
工作无法重新安装解析器。

文件图标与主题沿用既有声明。合并式的旧 `languages` 声明会被拒绝：请使用上面的独立数组，并
通过版本化的 LSP 能力声明语言服务。识别与高亮绝不会启动原生进程。
