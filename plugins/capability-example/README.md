# Capability Example

The panel menu explicitly offers the execution-budget and memory-budget fault demonstrations. Use an isolated fixture profile: each command deliberately traps this example instance so the native fault reminder and resource cleanup can be inspected.

版本 0.15.7 的普通 `preview-probe` 命令接收完整公开 `ui::Document` 并原样发布到 welcome 面板，用于独立检查版本、能力与权限的发布门禁；它不修正或伪造 source。组合 UI 资产的 `code_highlighting` 在没有 Preview source 时暂时关闭，绑定版本后恢复原声明。代码高亮消费者需要协商 `ui.code_highlighting`、`ui.richtext` 与 `editor.documents` 并获得 `editor.read`；普通等宽代码保持默认惰性。

0.15.6 使用当前独立 SDK 验证 `editor.navigation`／`ui.links`。公开 `scope-probe` 能发起版本化通用导航；测试重新打包为另一身份，验证非 Markdown 文档、权限、场景归属及取消。默认示例不增加浏览器权限，导航按测试夹具的声明与批准执行。

0.15.5 增加独立 `image-input-fixture` 的 `editor.images` 验收：组合树在没有 `Document.source` 时隐藏图片输入声明；原生图片通知只传元数据，示例使用公开 `scope-probe` 发起保存并展示异步回执。测试覆盖权限、跨实例句柄、名称边界、配额、碰撞重试与源版本撤销；默认包不增加图片写入权限。

0.15.4 增加独立 `images-fixture` 图片资源验收。组合 UI 资产中的 Image 在没有 `Document.source` 时仅发布替代文字，原模板保留，收到版本化 Preview 后声明资源。实际 ZIP 验证 `ui.images` 协商、逐图权限错误、本地工作区边界与异步 HTTP 回收；默认示例权限和界面不变。

0.15.3 增加独立 `edit-fixture` 的版本化选区与源码工具栏验收。组合 UI 资产可带 `editor_toolbar`，输出克隆在尚无 `Document.source` 时隐藏它，原始模板保留；测试在版本化 Preview 后通过原有 zoom 按钮回调验证工具栏事件。组合界面按当前 ID 消费 Dialog/Menu 的 Dismiss，保留 Dialog 的输入优先级和源码版本。范围编辑使用公开 scope-probe 与异步编辑请求，不附加测试专用宿主 API。默认示例界面和权限声明不变，验收包显式协商 `editor.edit` / `editor.toolbar`。

0.15.2 增加独立 `richtext-fixture` 打包验收：测试仅协商 `ui.native`、`ui.richtext` 与资源/配置能力，预览映射再申请 `editor.documents` 和 `editor.read`，不引用 Markdown 插件。组合树资产可携带富文本、代码块与源码字节范围；首次收到带版本的预览前，输出克隆移除未绑定的范围，原资产模板保留。`composable_ui` 集成测试覆盖能力声明与缺失拒绝、中文 UTF-8 范围、倒置及超配额范围。

0.15.0 增加 `interactive.execute` 1.0 的独立替代提供者夹具：消费者继续使用 `service-open` / `service-call`，测试打包时声明同一契约与权限。替代实现通过 stdio 执行 argv 并请求自身普通面板；不引用终端 ID 或终端源码。默认包仍不申请执行权限，也不发布这项契约。实际进程、选择、取消、来源授权和替换验证见 `interactive_execution` 集成测试。

0.14.0 增加 `ui.clipboard` 与 `storage.editor` 验收：使用类型化编辑器请求读写剪贴板、打开自身私有配置，并验证实际原生完成结果。

宿主通过 `Prepare` 交付的 opaque snapshot 会由 `Snapshot` 原样返回，用于验证重装、项目切换和历史数据导入不会清空插件私有状态。

0.11.0 增加 `MigrateData` 夹具：版本化包将私有值转换为 `v2:原值`，迁移期间验证无法访问工作区。
验收包通过 `migration-policy.txt` 选择正常迁移、迁移拒绝或激活失败，宿主不包含这些业务分支。
SDK `MIGRATION.md` 说明私有数据版本、隔离权限、最终副本与中断恢复规则。

0.10.0 增加 `fault-spin` 和 `fault-memory` 故障夹具，分别触发执行预算和内存预算。
插件管理页可查看原因并独立重启；恢复失败不会删除私有文件。具体限制见宿主导出的 SDK `FAULTS.md`。

0.9.0 增加公共服务契约验收：`service-open`、`service-call`、`service-cancel` 通过 SDK
发现、调用和取消服务。验收打包器仅修改声明与标签资源，即可构造消费者和可互换提供者；
默认示例包不申请服务调用权限。提供者示范参数/结果校验、来源权限、循环拒绝、显式资源释放
和异步授权延续，接口详见宿主 SDK 的 `SERVICES.md`。

0.8.0 提供组合 UI 示例：把 `label` 显式设置为 `composable-ui`，即可使用原生按钮和输入框操作 SVG 画布。`ui.canvas` 是可选能力，普通文本界面不需要它；示例不启用字符网格。`composed-ui.json` 通过公开打包脚本随 WASM 一起携带。`ui-layout` 命令接受 `form`、`canvas`、`combined`，示范同一通用布局树的三种用法。预览验收包声明编辑区面板，使用带版本的内存文档通知，回传 `Document.source`；不读取磁盘来替代未保存内容。

开发验证插件，演示独立能力版本、类型化文件及编辑器访问和原生文本界面。基础 API、package.assets、ui.native、workspace.files、storage.private、editor.documents、ui.panels、configuration 分别协商 1.x；不存在的可选接口会降级显示。0.5.0 默认每个工作区独立实例。

进程回归通过相同公开打包入口追加 `process` 协商与独立服务/执行权限，使用 `scope-probe`
发送类型化操作。`process-events` 按序号查看有界事件记录，`close-on-output` 演示在首个
输出回调中释放进程。普通示例包默认不申请原生执行权限。

安装时需要批准 assets.read（读取包资源）、workspace.read（读取所属工作区）、storage（读写实例私有文件）、clipboard（类型化剪贴板读写）、editor.read（选区和文档事件）、editor.write（保存已打开文档）与 ui.panels（自身面板显隐）。不申请进程或网络权限。菜单命令“检查类型化错误”验证未知操作、错误参数与路径越界的明确返回。

scope-write / scope-read 将工作区的 source.txt 与私有 value.txt 一起显示；scope-probe 接收公开 Operation JSON，并将 SDK 的类型化结果显示为文本。替换文本或剪贴板写入操作可追加 `repeat_text` 整数，在 guest 内重复请求文本，结果最多 1 MiB + 1 字节，以验证文本配额；命令参数、WASM fuel 和内存限额保持不变。诊断命令验证跨实例句柄拒绝、应用级实例不具有工作区权限，以及显式释放后的句柄失效。

SDK 提供 open_workspace、open_data、read_file、write_file、close_resource；句柄由宿主签发，不应持久化。workspace.files 1.0 只读，storage.private 1.0 支持私有根目录直接子文件的原子写入，每文件最多 1 MiB，累计受清单 storage_limit 约束。用户设置、其他工作区数据及宿主快照均不在可读根目录内。

通过宿主公开的 --plugin-cargo 入口构建，不使用宿主业务源码路径。开发打包脚本为 build-capability-example.ps1；输出仅用于新平台迁移验证，不纳入正式发行包。

## 编辑器请求与事件

- “读取选区及文档版本”显示选中文字并保存文档标识；“保存已读取版本的文档”仅保存该打开实体的相同版本。编辑、重命名、关闭后旧版本失败，不会跟随当前焦点保存其他文档。磁盘冲突须由用户解决。
- “读取活动目录”返回工作区相对目录；无活动文档时返回空字符串，代表工作区根目录。
- “隐藏自身面板”回收停靠空间；可从编辑器插件命令菜单重新“显示自身面板”。插件不能指定其他包的面板。
- “订阅文档变化”显示版本通知，“释放文档订阅”停止投递。溢出显式显示失败，用户再次点击订阅即可恢复。
- “取消当前请求”显示实际结果。尚未进入提交阶段时为 NotExecuted；原子提交开始后只能 WaitingStopped，不承诺撤回写入。StopWaiting 和 TryTerminate 在这些原子操作中有相同边界；后者不意味着保证终止。准备阶段可能生成临时文件和历史快照，取消不会清除已有历史快照。

`api::guest::EditorTask::start` 接受类型化 EditorOperation 和截止时间，`update` 自动关联通知并过滤其他任务、终态后的结果。示例只保存最新 UI 意图的 EditorTask，因此旧请求不能覆盖新请求结果。界面分别显示 Accepted、Progress、Completed 或 Cancelled；保存后台准备后才尝试提交，即时操作可能直接完成。

每实例最多 32 个未结请求、8 个订阅；每个文档队列最多 64 个待处理实体，按实体合并为最新版本并公平分批投递。跨批次保留最多 1024 个实体的版本水位；容量耗尽以 LimitExceeded 终止订阅，需重新订阅。通知是版本提示而非完整文本增量日志，不承诺重放订阅前的变化。最终请求结果保留至投递或实例退出，不作为可丢弃进度处理。停用、卸载或撤销信任统一撤销实例资源；已停用 guest 不再接收回调。

## 声明式配置

独立 SDK 夹具可用 `preview-probe` 发布原样的 source-bound Scroll，并通过 `scope-probe` 发出公开 `LocateViewport`；无 source 时视口 opt-in 撤销，不制造虚假的文档身份。`editor.viewport` 为可选协商，普通示例保持惰性。

编辑器“设置 → 插件设置”根据清单生成 enabled（布尔）、label（文本）、count（1–20 的整数，仅用户级）、style（枚举）的原生表单。选择用户全局或本项目后点击应用才写入；重置移除所选层级覆盖。每项显示当前生效来源。配置顺序为已确认项目值 → 用户值 → 自动发现 → 默认值；项目不能写宿主信任、授权或主题等设置。

示例钩子在 Validate 阶段提供 label 的自动发现值 `Discovered label`；显式 label 为 `invalid` 时拒绝应用，保留之前实例和配置。显式有效值不会被发现值替换。Apply 阶段把最终配置送入候选实例，激活后显示 enabled 的实际值。清单声明 `restart_instance`，修改仅替换受影响的实例，不重启编辑器。全局更改也更新其他已打开逻辑工作区，但保留其项目覆盖。

禁用插件时修改配置只执行受限的准备/校验，不激活插件；启用后采用已保存值。普通卸载保留配置，选择删除数据时删除配置。该示例仍通过公开 SDK 构建，不访问宿主业务源码。

## 运行与调试公开接缝验收（0.16.0）

消费者通过 `service-open`、`service-call` 和 `service-cancel` 使用 SDK 中的 `interactive.execute` 2.0、`session.host` 2.0 与 `debug.session` 1.1。真实执行夹具以公开的 `input`、`events`、`locate` 和 `stop` 方法验证输入归属、输出/状态订阅、隐藏后的定位、正常退出与强制终止；不读取终端私有进程句柄。

`service-forge-host-reference` 在 guest 可见的资源句柄上伪造宿主实例字符串，验证宿主拒绝伪造来源。它不改变宿主私有引用字段或授权规则。订阅夹具验证跨来源、已关闭引用和配额耗尽；调试声明夹具只证明契约接入，真实 Rust 断点与步进由独立 Rust 调试提供者验收。
