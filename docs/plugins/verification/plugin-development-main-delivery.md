# 缺失项目描述修复与主工作区 Release 交付

日期：2026-10-09。用户要求在 `codex/plugin-development` 工作区修复截图错误，整合到主工作区后重新启动，并明确指定 Release 版本。

## 原因与反馈循环

截图选择 `C:/Projects/RustProjects/Editor/plugins/terminal`，但该主工作区尚未包含本分支的 `nanobug-plugin.json`。目录与测试工作区均存在；配置验证在读取项目描述的 metadata 时直接传播 Windows `os error 2`，没有说明缺失的是哪个文件。不是编译器、原生桥或测试工作区准备失败。

- 在插件开发工作区执行 `target/debug/editor-app.exe --plugin-build C:/Projects/RustProjects/Editor/plugins/terminal`，连续两次退出 1，输出只有系统找不到文件。
- 新增 `missing_project_description_is_actionable_and_recovers`，通过宿主配置验证接缝重现同一错误；修复前失败，断言输出没有 `nanobug-plugin.json`。
- 运行时保留描述路径和原始 I/O 错误类型；宿主针对缺失描述提供中英文提示，其他文件访问／格式错误仍保留原因。读取不会自动生成配置、选择其他工作区或启动构建。
- 同一回归测试覆盖英文与中文，并在补齐项目描述后确认同一配置恢复有效。运行时另测缺失描述与非法 JSON 的诊断区分。

## 备份与范围

修复前的两侧未提交文件备份到 `C:/Users/Tmiracle/.codex/workspaces/Nanobug/plugin-development-to-main-20261009/`，`before.json` 记录 392 个路径。`before-feature/` 与 `before-main/` 保留两侧原始字节；本次整合以 2026-10-08 导入时保存的主工作区内容为三方基线，保留后续主工作区改动。

本机日志目录为 `C:/Users/Tmiracle/.codex/workspaces/Nanobug/`；备份与日志不是仓库打包依赖。没有执行仓库或 Codex 存档脚本，没有安装工具或修改全局 PATH。

## 针对性验证

- `cargo test -p plugin-runtime --lib development::tests -- --test-threads=1`：8 通过、0 失败。
- `cargo test -p editor-app --bin editor-app plugin_development -- --test-threads=1`：6 通过、0 失败，包含真实 GPUI 模板与新增双语恢复回归。
- 日志：`plugin-development-missing-project-red.log`、`plugin-development-missing-project-red-repeat.log`、`plugin-development-description-red-test.log`、`plugin-development-description-runtime-tests.log`、`plugin-development-description-app-tests.log`。

## 主工作区整合与 Release

主工作区 `main` 已整合插件开发工作区的内容：97 个路径更新，296 个路径保留主工作区内容，三方整合无冲突。两侧已有改动均保留为未提交状态；这次没有创建合并提交、推送或关闭议题。

- 两侧 `cargo fmt --check`、`cargo test --workspace --exclude editor-app` 与 `cargo check --workspace` 均通过。主工作区非 UI 测试合计 212 通过、175 ignored；ignored 项没有计入通过数。
- 主工作区插件开发 UI 针对性测试再次执行，6 通过、0 失败。站点 `npm run build` 通过，包含 17 项内容测试。
- 主工作区 `cargo build -p editor-app --release` 通过，优化构建耗时约 69 秒。使用已安装的 MSVC／Windows SDK，仅为构建进程设置库搜索路径，没有安装工具或修改全局环境。
- 使用主工作区 Release 宿主执行 `--plugin-build C:/Projects/RustProjects/Editor/plugins/terminal --release` 成功；随后不添加构建环境变量、使用默认访客配置执行同一路径的 `--plugin-build` 也成功。两次均实际完成 terminal WASM 构建，缺失描述错误已消除；此命令生成开发候选目录，不生成 ZIP。
- 已直接启动 `C:/Projects/RustProjects/Editor/target/release/editor-app.exe C:/Projects/RustProjects/Editor`，进程 PID 为 32604。原生窗口标题为 Nanobug，主工作区文件树与文档预览正常显示。本次原生观察确认启动；配置提示的双语与恢复行为由上述 GPUI 回归测试验证，没有将未操作的配置弹窗宣称为人工验收通过。

主工作区日志：`plugin-development-main-format.log`、`plugin-development-main-app-tests.log`、`plugin-development-main-workspace-tests.log`、`plugin-development-main-check.log`、`plugin-development-main-release-build.log`、`plugin-development-main-terminal-green.log`、`plugin-development-main-terminal-default-green.log`、`plugin-development-main-release.stdout.log` 与 `plugin-development-main-release.stderr.log`。本次没有重新验收安装器或其他平台。
