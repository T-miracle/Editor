# 运行时插件平台

主程序只运行当前能力协议（清单 protocol 7、api.base ^1）。插件使用声明式贡献，必要时附加 WASM 生命周期或策略钩子。语言识别、高亮、LSP、UI、进程和插件间服务按声明与能力协商，不按插件名称选择实现。

## 安装与热生效

标题栏“插件管理”提供已安装列表、本机随附市场、README、安装、更新和卸载。确认来源、权限及所需原生安装步骤后，成功安装即可启用，无需重启编辑器；全局禁用时可明确在当前项目启用。LSP 依赖未就绪时单独显示原因，不把安装成功当作服务就绪。

语言、主题、图标与面板随安装、启停、更新和卸载同步到已打开的文档。面板隐藏、停用和卸载会释放原生输入目标并回收停靠空间。插件命令按清单注册；未安装或未启用时不会偷偷安装或扩大权限。

## 公共接口

WIT 世界 `editor:plugin/plugin@0.1.0` 提供 host.request 导入与 dispatch 导出。Rust SDK 0.2.0 使用下列公开类型，WIT 版本与各能力 SemVer 独立：

| 方向 | 契约 |
| --- | --- |
| 宿主 → 插件 | Invocation：Prepare、Activate、Event、Snapshot；事件含可选面板身份及 Notification |
| 插件 → 宿主 | Completion：请求 ID 对应的 Output 或 Failure |
| 插件请求宿主能力 | api::Request / Operation → Response；耗时请求先受理，再通过 Notification 返回最终结果 |
| 插件界面 | Output.views → 每面板 ui::Document；稳定节点 ID 与 revision 约束输入 |
| 插件间协作 | 版本化服务契约、方法结构与来源权限；消费者引用契约，用户可替换提供者 |

不再存在旧 Message/Reply/Scene、字符串宿主命令、chrome/controls 兼容字段或运行时转换。标准组件、SideTabs 与 Canvas 组成同一棵 UI 树，宿主的 gpui-base 行为和本地外观负责输入、焦点、布局、滚动、菜单及弹窗。SVG 预览接收带 DocumentVersion 的内存文本，必须回传 source，包含未保存编辑；过期结果不能覆盖新文档。

详细接口见 [SDK](../../crates/plugin-protocol/README.md)、[UI](../../crates/plugin-protocol/UI.md)、[语言](../../crates/plugin-protocol/LANGUAGES.md)、[LSP](../../crates/plugin-protocol/LSP.md)、[进程](../../crates/plugin-protocol/PROCESSES.md)、[服务](../../crates/plugin-protocol/SERVICES.md)。

文件预览可以通过公开 `editor.presentation` 声明源码／分栏／预览三态，通过 `editor.toolbar` 提供源码顶部操作；版本化范围编辑、图片输入、导航及视口请求分别核对文档身份、revision 和来源权限。Markdown 使用这些通用接口提供实时原生预览、任务框写回、同级图片导入和双向内容块滚动；SVG 使用同一底栏三态入口。详见[Markdown 使用说明](../../plugins/markdown/README.md)、[导航](../../crates/plugin-protocol/NAVIGATION.md)、[视口](../../crates/plugin-protocol/VIEWPORT.md)和[代码块高亮](../../crates/plugin-protocol/CODE_HIGHLIGHTING.md)。新增插件仍只使用公开契约，不需要宿主按插件名称分支。

## 权限与原生执行

WASM 不继承宿主目录、环境、输入输出或网络访问。每次调用检查协商能力、安装授权、实例作用域与活句柄。包资源只读；工作区与私有数据使用受限根句柄，拒绝越界路径、设备路径、备用数据流与链接逃逸。工作区受限时撤销运行资源。

| 权限 | 范围 |
| --- | --- |
| assets.read | 当前包的只读资源 |
| workspace.read / storage | 获授工作区读取 / 当前实例私有数据 |
| editor.read / editor.write | 读取版本化文档信息 / 保存明确版本的文档 |
| ui.panels / clipboard | 自己声明的面板 / 原生剪贴板 |
| process.service.<id> | 清单固定服务；不能任意替换程序或参数 |
| process.exec | 使用程序路径与 argv 执行任意本机程序，支持 stdio 或 PTY |
| services.call | 调用声明的插件服务；完整调用链仍受来源权限约束 |
| dependencies.prepare / dependencies.install | 校验后准备私有依赖 / 经额外确认执行声明安装步骤 |

原生程序以当前 OS 用户身份运行，具有该用户的文件、网络与进程权限；WASM 权限不构成子进程文件沙箱。宿主经工具链解析程序并传 argv，不拼接 Shell 命令。插件自己的交互式 Shell 输入是用户授权的终端行为。

依赖只在获授权的安装/更新流程中下载、校验并写入私有缓存；原生安装步骤另行展示工具、参数及用途。不静默修改全局环境。取消只能停止尚未执行的请求、停止等待或尽力终止进程；不能承诺回滚已发生的外部副作用。

## 更新、故障与数据

1. 后台编译、验证候选包和私有依赖，旧实例继续处理输入。
2. 进入切换阶段，撤销旧异步请求与服务租约，获取最终快照和数据。
3. 在隔离副本执行数据迁移；仅允许访问包资源和私有副本。
4. 候选激活成功后以日志、备份和原子替换提交包记录与私有数据，再发布新实例。
5. 准备失败保留旧实例；切换失败恢复旧版本及数据并分配新实例身份，旧句柄和回调不会复活。

备份及恢复失败路径、休眠工作区升级见 [迁移契约](../../crates/plugin-protocol/MIGRATION.md)。快照是插件的逻辑数据，不能恢复 OS 进程内存或撤销用户文档、外部文件等副作用。终端恢复有界历史和新 Shell；经 interactive.execute 服务创建的执行会话只恢复历史，不重跑程序。

WASM 指令、时间和内存有独立预算；队列、进程、文件、绘图和事件均有上限。超限定位到插件/作用域/操作；插件可独立重启，LSP 有限重试。停用与卸载结清请求、撤销订阅和租约并回收进程树。

默认数据根为 %APPDATA%/MeEditor/runtime-plugins，ME_EDITOR_PLUGIN_HOME 可指定隔离测试根。registry.json 保存安装与启用偏好，packages 保存不可变代码，scope 私有数据由宿主管理。历史 ID/目录只执行有限导入并保留备份。协议 1–6 的安装记录可展示更新、卸载及保留的偏好，但不运行旧代码；重装新版可导入保留数据。

## 构建与验证

当前构建通过实际主程序 CLI，归档直接使用 ZIP 工具，步骤见[原生打包说明](../../installer/README.md)。仓库旧辅助脚本已移出，打包不调用这些脚本。

插件依赖主程序内嵌的 SDK，不引用宿主源码或复制 vendor。`--plugin-cargo` 按内容摘要管理接口缓存，插件声明 `plugin-protocol = { version = "=0.2.0", features = ["guest"] }`。显式 --export-plugin-sdk 供其他工具链使用；使用已编译插件无需 Rust/Cargo。

```powershell
cargo build -p editor-app
# LSP、安装器与进程契约使用本地原生夹具。
cargo build -p plugin-runtime --examples
.\target\debug\editor-app.exe --plugin-cargo plugins/terminal/Cargo.toml build --target wasm32-wasip2 --release
.\target\debug\editor-app.exe --plugin-cargo plugins/capability-example/Cargo.toml build --target wasm32-wasip2 --release
.\target\debug\editor-app.exe --export-plugin-sdk .\target\sdk-export
cargo fmt --check
cargo check --workspace
cargo test --workspace --exclude editor-app
# 原生集成共享全局语言注册表，按顺序运行。
$env:RUST_MIN_STACK = '16777216'
cargo test -p editor-app --bin editor-app -- --test-threads=1
cargo test -p plugin-runtime --tests -- --ignored --test-threads=1
# 先结束 Cargo 再运行真实语言分析，避免 rust-analyzer 等待父 Cargo 的 target 锁。
$artifacts = cargo test -p editor-app --bin editor-app --no-run --message-format=json
$testExe = $artifacts | ForEach-Object { $_ | ConvertFrom-Json } |
    Where-Object { $_.reason -eq 'compiler-artifact' -and $_.profile.test -and $_.executable } |
    Select-Object -ExpandProperty executable -Last 1
& $testExe --ignored --test-threads=1
```

发行包为 terminal、example、svg、rust、toml、html、javascript、markdown；每包包含 README。Rust 包附带公开 LSP 钩子，仅提供语言资源的包无需空生命周期组件。Markdown 包同时包含两份 WASM grammar 和独立解析组件。终端直接使用上游 term-wm-vt100，许可随包分发；基础主题内置。

Windows 上执行实际 WASM、GPUI、ConPTY、进程树和语言服务验收。macOS/Linux 未在本次环境实测或交叉构建，不以 Windows 结果替代其他平台验证。完整矩阵及日志见 [最终契约验收](verification/plugin-api-contract-verification.md)。
