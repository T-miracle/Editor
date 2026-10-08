# 插件打包与隔离开发运行验收

日期：2026-10-08。对应[实施规格](../specs/plugin-development-packaging.md)，范围为 Windows 实际构建与原生交互、macOS/Linux 接口兼容。未提交、推送或发布。

## 工作区与依据

- 工作区：`C:/Users/Tmiracle/.codex/worktrees/plugin-development/Editor`。
- 分支：`codex/plugin-development`，提交基线 `ebe40c270f2ceb616b12eca0125b461648ce4548`。
- 初始实现未移入原主工作区的未提交 UI、安装器和其他改动，仅继承仓库协作规则、必读基线文档及 13 个脚本删除。后续用户授权整合全部未提交改动，范围、备份与重新验证见[主工作区整合记录](plugin-development-main-integration.md)；继承项不计作本次新增功能。
- 未调用原 `scripts/` 或 Codex 工作区中的归档副本，未安装工具链或改动全局 PATH。
- 日志与独立夹具位于 `C:/Users/Tmiracle/.codex/workspaces/Nanobug/`。这只是本次本机证据位置，不是产品依赖。

## 自动验证

| 实际命令 | 结果 |
| --- | --- |
| `cargo fmt --check` | 通过 |
| `cargo test -p editor-app plugin_development` | 5 通过、0 失败、0 跳过；包括无插件 GPUI 模板、可见表单高度、Apply、输入替换版本、独占 profile 租约与重载身份／权限门禁 |
| `cargo test -p editor-app sdk_export` | 6 通过、0 失败、0 跳过；包括 4 个同步启动的 SDK 导出者、缓存复用及损坏修复、导出文档链接 |
| `cargo test -p editor-app run::` | 103 通过、0 失败、11 ignored；未执行的真实工具／交互夹具不计为通过 |
| `cargo test --workspace --exclude editor-app` | 203 通过、0 失败、169 ignored；其中 7 项新 builder 单元测试实际执行 |
| `cargo check --workspace` | 通过 |
| `cargo build -p editor-app` | 通过；最终可执行程序用于重启验收 |
| `npm ci`、`npm run build`（`website/`） | 站点构建通过；17 项正文、双语、链接及搜索测试通过，0 跳过 |
| 本地 Markdown 路径检查、`git diff --check` | 120 个本地链接解析成功；差异空白检查通过。继承说明中的旧 SDK、安装器和历史验收链接已改为现有入口 |

最终日志为 `plugin-development-final-format.log`、`plugin-development-final-app-tests.log`、`plugin-development-final-sdk-tests.log`、`plugin-development-final-run-tests.log`、`plugin-development-final-workspace-tests.log`、`plugin-development-final-check.log`、`plugin-development-final-build.log`。既有未使用代码／链接器警告仍存在，本次未顺手修改。

Builder 回归覆盖项目根目录默认输出、显式目录、同名原子替换、资源缺失保留旧 ZIP、输出位于资源目录而不嵌套自身、取消、重复目标、越界来源、平台不匹配，以及目录与 ZIP 相同准入规则。

## 真实插件与独立 SDK

通过宿主 `--plugin-package` 实际构建所有 14 个项目并输出至 `dist/plugins/`，各项目独立成功。包括声明式资源、Rust WASM、terminal 原生桥接程序及分发清单内的内容哈希。完整日志：`plugin-development-all-packages.log`。

| 插件 | 包版本 |
| --- | --- |
| capability-example | 0.16.2 |
| configuration-example | 0.1.2 |
| example | 0.3.2 |
| html / javascript / toml | 0.2.2 |
| layout-example | 0.3.1 |
| markdown | 0.12.1 |
| run-target-example | 0.1.0 |
| rust | 0.5.1 |
| rust-debugger | 0.1.1 |
| svg | 0.5.1 |
| terminal | 0.12.2 |
| tools-example | 0.1.1 |

本次手动更新了实际修改包资源的清单版本及对应 `plugin.toml`；打包流程本身不会改版本。capability-example 在补充 README 之后用最终宿主再次打包成功，日志 `plugin-development-final-capability-package.log`。

根目录默认输出已通过 `plugins/toml`、`plugins/html` 实测。混合一个不存在项目的批次仍完成其他项目，整体退出码为 1，日志 `plugin-development-batch-failure.log`。

独立项目仅复制自身源文件、Cargo 清单、插件清单、资源及项目描述，不带仓库 `crates/` 或 SDK 源码：

- `sdk-independent-example`：`--plugin-build` 成功，返回目录候选，无 ZIP；日志 `plugin-development-independent-build.log`。
- `sdk-independent-capability`：`--plugin-package` 成功，ZIP 在该项目根部；日志 `plugin-development-independent-sdk.log`。

显式执行实际 WASM 测试，未把默认 ignored 计作通过：

```powershell
# 指向 --plugin-build 打印的实际目录，再执行真实重载与权限回归。
$env:NANOBUG_DEVELOPMENT_CANDIDATE = '<实际目录候选>'
cargo test -p plugin-runtime --test development_projects -- --ignored

# 指向独立 capability 项目的实际 ZIP，执行既有 SDK 行为验收。
$env:NANOBUG_SDK_PACKAGE = '<独立项目>/capability-example-0.16.2.zip'
cargo test -p plugin-runtime --test sdk_distribution -- --ignored
```

分别 2/2、1/1 通过（`plugin-development-real-tests.log`、`plugin-development-sdk-real-test.log`）。前者验证真实组件增量重载保留逻辑快照、无效组件激活回滚、重开 profile 保留数据、另一 profile 隔离，以及权限增加被拒绝；后者验证完整 SDK 导出和组件实际调用的错误类型、关联服务与原生 UI 结果。

## Windows 原生交互

使用新分支 Debug 可执行程序和临时 profile，实际观察 GPUI 窗口并输入、点击系统目录选择框；没有操作权限或信任提示。

1. 在未安装插件的主验收实例中，“添加 → Nanobug”始终显示“插件打包”“插件调试”。创建草稿并通过保存提交。
2. 打包配置选择 TOML 插件项目。通过 Windows 系统目录选择框改为 `plugin-development-gui-zips`，输入框立即显示新路径。Run 后输出 `OK ...toml -> ...toml-0.2.2.zip`，状态由启动中变为已结束，真实 ZIP 为 12,998 字节。
3. 调试配置可选测试工作区与自动监听；留空工作区使用插件项目。Build 输出目录候选，未启动额外开发窗口。源码 Debug 禁用，提示当前模板提供开发运行和日志、不支持 WASM 源码断点。
4. 浅色和深色主题实际检查表单、按钮、输入、说明与滚动。通过键盘全选与中文文字输入把配置名称改为“插件调试验收”，名称和配置树立即更新，保存后工具栏显示该名称。
5. CLI 运行独立 example 项目，profile 为 `plugin-development-cli-profile`，默认工作区为插件项目。加载 13 个随附插件及同 ID 的开发 example；主验收实例同时保持自己的 TOML 工作区和运行配置。
6. 在开发计数器中实际点击“增加一次”，显示 1。修改独立项目 README 触发 `--watch`，输出 admitted/activated，计数器仍为 1；标准输入 `reload` 再次成功。
7. 把资源改为不存在路径，日志显示 `Reload failed; previous version retained`；还原后再次 admitted/activated。同 profile 第二个控制器退出码 1，明确拒绝独占租约。
8. 标准输入 `stop` 后控制器退出码 0，所属开发窗口和进程退出。已构建的宿主再次启动同 profile，开发计数器仍为 1；再次 Stop 退出码 0。退出后的重载候选不会再次激活旧会话。

控制器日志：`plugin-development-controller.log`；租约冲突：`plugin-development-profile-collision.log`；最终宿主重启：`plugin-development-restart.log`。验收结束时关闭自建测试进程；保留独立 profile、夹具和日志供复查。

## 验收中修复的缺陷

- 表单滚动根节点的百分比高度导致子输入存在却被零高度裁切，改为可增长的根节点并增加可见高度测试。
- 系统选择器值已保存但输入显示未更新，按字段推进原生输入替换版本，普通输入保留稳定版本与编辑状态。
- 深层 Windows 工作区触发 MSVC 原生产物路径长度错误，改用用户缓存目录中的短项目散列路径。
- 声明式包生成错误的 `services: null`，改为仅处理实际服务对象。
- 并发首次 SDK 导出遇到 Windows 原子替换冲突；相同内容胜者视为成功，其他写入错误保留明确路径，新增并发回归。
- 重置使用同一跨进程 profile 租约，避免仅检查当前窗口的 Job 而漏掉 CLI 控制器。
- 候选重载在控制器和实例准入两侧检查相同 ID 与既有授权，改 ID 需重新启动配置，不会把两个开发身份同时留在实例内。

## 保留边界

macOS/Linux 没有本机运行或交叉编译验收，只交付兼容路径、进程及平台声明接口。WASM 源码断点不在范围内。未操作 GUI 信任／权限授权提示；CLI 显式权限门禁和真实管理器权限回归已验证。中文验收为提交的字符输入，未穷举输入法候选、所有 DPI／缩放组合。既有其他 ignored 夹具未执行，不称全部 UI／全平台测试通过。

首次初始化所有随附 WASM 插件在 Debug 宿主上较慢；启动准备在既有后台 actor 中执行，初次加载耗时不承诺固定上限。打包失败、缺失工具和新增权限均报告错误，不安装整个项目工具链。
