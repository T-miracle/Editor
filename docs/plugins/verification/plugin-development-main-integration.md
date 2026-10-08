# 插件开发工作区整合主工作区改动

日期：2026-10-08。用户要求拉取主分支最新改动，随后明确授权整合主工作区全部未提交改动，同时保留插件开发实现、不修改或提交主工作区。

## 提交与备份

- 执行 `git fetch origin` 成功。`origin/main`、本地 `main` 与 `codex/plugin-development` 都是 `ebe40c270f2ceb616b12eca0125b461648ce4548`，没有新的远端提交。
- 新工作区最初从已提交的 main 创建，Git 不会带入主工作区的未提交文件。这次按共同提交三方整合磁盘上的改动，没有创建提交或推送。
- 来源为 `C:/Projects/RustProjects/Editor`：169 个 tracked 改动、157 个 untracked 文件。
- 目标为 `C:/Users/Tmiracle/.codex/worktrees/plugin-development/Editor`：整合前 69 个 tracked 改动、40 个 untracked 文件。
- 两侧原始文件与共同基线备份到本机 Codex 工作区 `C:/Users/Tmiracle/.codex/workspaces/Nanobug/main-integration-20261008/`，`plan.json` 保存每个路径的来源及处理方式。该位置不是构建依赖。
- 初步三方比较得到 230 个导入、6 个自动合并、52 个删除、20 个需人工处理的冲突，其余路径保留。最终字节核对显示来源内容未变、插件开发专有文件未丢失。

## 冲突与运行产物

- 宿主启动保留插件开发 CLI 和隔离 profile，同时接入安装器 mutex。Worker 同时保留事务重载入口、几何合并和过期视口处理。SDK 导出保留打包文档、并发发布，同时导出新的增量 UI 与可视视口模块。
- Markdown 保留来源清单、Cargo 与资源版本 0.17.0；Image 保留 0.6.0 及 `ui.viewport` 声明。没有以开发分支的旧版本覆盖它们。
- 双语 README 保留 Windows 安装器说明与自动 ZIP／隔离开发入口。安装器文档接入 `--plugin-package` 和带版本的包文件名；导入文档的失效 SDK 链接修正到站点源，原有验收命令作为历史证据保留。
- 保留全部脚本删除，仅移除文件已删后的空目录；没有执行仓库或 Codex 存档脚本。
- 用新宿主重新打包全部 14 个项目。`dist/plugins/` 保留 14 个当前 ZIP，旧 `markdown-0.12.1.zip`、`svg-0.5.1.zip` 移到上述备份的 `old-dist-plugins/`，避免开发实例扫描到重复旧身份。

## 本次验证

| 命令或检查 | 实际结果 |
| --- | --- |
| `cargo fmt --check`、`git diff --check` | 通过 |
| `cargo check --workspace`、`cargo build -p editor-app` | 通过 |
| `cargo test --workspace --exclude editor-app` | 211 通过，175 ignored；跳过项不计为通过 |
| `cargo test -p editor-app --bin editor-app -- --test-threads=1` | 430 通过，168 ignored；包括插件开发、SDK 导出及运行配置回归 |
| `npm run build`（website） | 构建通过，17 项测试通过 |
| 新宿主 `--plugin-package`，全部 14 个仓库项目，`--output dist/plugins` | 全部成功，包括 WASM、原生桥与资源包；Markdown 0.17.0、Image 0.6.0 |
| 新宿主 `--plugin-build` 独立 example、`--plugin-package` 独立 capability-example | 仓库外夹具成功，使用新宿主内嵌 SDK |
| `cargo test -p plugin-runtime --test development_projects --test sdk_distribution -- --ignored --test-threads=1` | 明确配置新目录候选和独立 ZIP 后，3 项真实 WASM 测试通过 |
| 改动 Markdown 的本地链接、备份字节与源码保留检查 | 无失效链接，来源无改动，插件开发专有文件无丢失 |
| 原生窗口检查 | 新构建打开目标工作区、显示更新的 README；配置弹窗的添加菜单显示“插件打包”“插件调试”。取消检查，不保存配置改动 |

首次非 UI 测试链接失败，原因是当前终端未配置现有 MSVC 库搜索路径，`link.exe` 找不到 `msvcrt.lib`。只为后续进程设置 `LIB` 为已安装 MSVC 14.50.35717 与 Windows SDK 10.0.26100.0 的 x64 路径后重跑通过，没有安装工具或修改全局环境。宿主测试使用 `RUST_MIN_STACK=16777216`。

证据在 `C:/Users/Tmiracle/.codex/workspaces/Nanobug/`，前缀为 `main-integration-`：`check.log`、`build.log`、`workspace-tests-sdk-env.log`、`app-tests.log`、`app-tests-sdk-env.log`、`website-build.log`、`package-tests.log`、`independent-build.log`、`independent-sdk.log`、`real-tests.log`、`format.log`、`diff-check.log` 与启动 stdout/stderr。

原有“插件调试”配置指向主工作区 `C:/Projects/RustProjects/Editor/plugins/terminal`，该处没有本分支的 `nanobug-plugin.json`，所以原生弹窗报告文件不存在；运行配置需选择当前插件开发工作区的项目。本次没有改写用户保存的配置或安装记录。

## 限制

本次没有重新验收 Windows 安装／卸载流程，也没有运行其余 ignored 测试；既有验收保留历史含义。原生检查限于启动、README 和模板菜单，不替代完整 IME、缩放、性能及重载交互矩阵。macOS/Linux 未构建、未实测。主工作区与本分支都仍有未提交改动，没有提交、推送或关闭议题。
