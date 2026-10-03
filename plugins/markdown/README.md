# Markdown 插件

本包通过公开语言贡献为 `.md`、`.markdown` 提供 Tree-sitter WASM 源码高亮，并通过文件预览贡献显示左源码、右原生预览。输入、粘贴、撤销、重做和重新加载后，预览采用当前未保存内容。停用、卸载与替换后撤销对应预览和语言贡献。

预览支持 CommonMark 基础语法及 GFM 表格、任务列表和删除线。列表保留有序起始编号，支持嵌套引用、列表和代码块；任务框当前只读，图片显示替代文字，代码块使用等宽文本。原始 HTML 显示为文字，不渲染嵌入 HTML；首版不提供数学公式和 Mermaid。

Markdown 解析在独立 WASM 组件中完成。宿主提供通用原生富文本、代码块和源码范围能力；插件不读取文档文件、不修改编辑器文本，也不维护另一份撤销历史。预览树回显源文档身份与 revision，以供宿主拒绝过期结果。

底栏左侧工具按钮组右边显示三个视图按钮，分别切换“仅编辑”“分栏”“仅预览”。首次使用默认左源码、右预览，分栏中间可拖动；选择按工作区记忆，同工作区切换 Markdown 文件及重新打开沿用最近模式。切换模式保留文档、选区及撤销历史，隐藏的一侧不留空白。

图标通过公开 `editor.presentation` 能力和面板 `view_modes` 声明提供。三个 SVG 分别以文本行、左右分栏和预览图案表达模式，采用 16 × 16 的设计网格和 `currentColor` 随主题着色，按项目底栏按钮的 14 px 尺寸显示；按钮提示、选中态和键盘焦点由通用宿主控件提供。

构建时先构建宿主，再执行 `./scripts/build-plugins.ps1 -Packages markdown -HostExe target/debug/editor-app.exe`。脚本使用宿主 `--plugin-cargo` 和版本化 SDK，生成 `dist/plugins/markdown.zip`；访客项目无需引用宿主源码路径。

当前交付至工单 03；格式工具栏、图片、任务交互、导航、高亮及同步滚动按[总方案](docs/spec.md)与[工单目录](docs/tickets/README.md)继续实施。同步滚动按钮将在工单 10 的实际双向同步行为完成时一同上线。

grammar 来源与资源 hash 见[说明](grammar/README.md)，包内保留上游 MIT 许可证。Markdown 解析使用 [pulldown-cmark 0.13.0](https://github.com/pulldown-cmark/pulldown-cmark/tree/v0.13.0)，包内保留其 MIT 许可证。
