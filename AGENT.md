# Me Editor 代理工作约定

开始实现前必须阅读 `docs/需求整理.md` 与 `docs/开发计划.md`，并以其中已确认范围为准。

## 架构

- 保持 `editor-app`、`editor-core`、`platform-windows`、`plugin-schema` 四个 crate；未经明确要求，不将每个逻辑模块拆为 crate。
- `EditorState` 是唯一内存文本真相来源。不得创建第二份可变 Rope 或 Undo/Redo 栈，也不得绕过 `DocumentSession` 写入文本。
- 文档、语言结果和替换计划必须使用 revision；过期异步结果必须丢弃。
- grammar 必须来自插件 WASM 动态加载；不得为高亮重新启用宿主内建 grammar。
- `plugin-schema` 仅承载稳定、声明式格式；插件不得引入原生代码或 JavaScript 运行逻辑。
- 外部工具只能经 `toolchains` 解析，并以可执行路径和参数数组启动；禁止 Shell 拼接命令。

## 安全与数据

- 受限工作区不得启动插件、语言工具或采用项目工具路径。
- 插件仅安装时展示权限；GitHub 安装锁定 commit/hash，首版仅公开仓库。
- 不自动下载、安装或升级工具和插件。
- 项目设置不得覆盖用户语言、主题和信任状态。
- 本地历史不得写入项目目录；编辑器不得保存 Git 凭据。
- 批量替换和破坏性 Git 操作必须先预览再确认。

## 变更与验证

- 不经用户允许，不提交 Git。
- 使用 `apply_patch` 编辑文件，保留用户既有改动。
- 每完成一个阶段至少运行：

  ```powershell
  cargo fmt --check
  cargo test --workspace --exclude editor-app
  cargo check --workspace
  ```

- 修改编辑、插件、工具链、搜索、历史或 Git 时，补相应回归测试。
- 先完成动态 WASM grammar 验证，再大规模接入插件或语言工具。

不要自行加入终端、任务执行、调试、市场、多窗口、自由停靠或私有 GitHub 插件支持。
