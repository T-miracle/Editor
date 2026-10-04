# 插件 SDK

源码视口通知、双向语义定位、来源抑制和工作区同步偏好通过 `editor.viewport ^1` 提供，见 [VIEWPORT.md](VIEWPORT.md)。

只读原生代码块的提供者高亮、授权、降级与资源生命周期见 [CODE_HIGHLIGHTING.md](CODE_HIGHLIGHTING.md)。

原生链接事件、文档／预览块／HTTP(S) 导航与权限契约见 [NAVIGATION.md](NAVIGATION.md)。

跨插件协作、版本化契约、提供者选择及来源权限见 [SERVICES.md](SERVICES.md)。

原生服务、交互式进程、权限与回收契约见 [PROCESSES.md](PROCESSES.md)。
提供运行与调试服务的提供者契约（interactive.execute 1.3、debug.session 1.0）、观察与清理语义见 [SESSIONS.md](SESSIONS.md)。

纯声明式语言包及独立识别、高亮提供者的格式见 [LANGUAGES.md](LANGUAGES.md)。无需附带空生命周期组件；安装、启停、更新和卸载会同步到已打开文档。

## 当前能力协议

协议传输标记 `protocol = 7` 选择 `api` 模块的类型化消息；它不是所有功能共用的接口版本。清单 `api.base` 使用基础协议 SemVer 范围，`api.required` / `api.optional` 分别声明能力 ID 与版本范围。当前基础 API 为 1.0.0，支持 `package.assets` 1.0.0 和 `ui.native` 1.0.0；插件包版本仍单独管理。必需接口缺失或版本不匹配在包检查及实例恢复时拒绝，可选接口不可用则不出现在 Prepare 的协商结果中。

插件使用 `api::guest::dispatch` 接收类型化生命周期，并用 `api::guest::read_asset` 读取资源。SDK 自动生成非零请求 ID、编码消息和校验响应 ID。缺少方法、错误参数、未知操作、未协商能力、权限不足、非法路径与配额超限以 `Failure` 返回；真正无法解码或调用的组件传输错误仍走 WIT 错误。同步资源读取直接返回最终结果，不伪造“处理中”阶段。

`package.assets` 的可用性不等于授权：清单还需声明并在安装时批准 `assets.read`。该操作只读取当前包版本内的资源，准备阶段也允许读取这些不可变资源；不能读工作区或其他插件目录。

新 UI 输出使用 `api::View { panel, document }`，普通表单不要求画布或字符网格字段。宿主检查 `ui.native` 是否已协商、面板是否已声明；`ui.canvas ^1` 允许在同一树任意位置放置画布，`ui.grid ^1` 单独提供可选字符测量。`ui.richtext ^1` 提供只读原生富文本、等宽代码块和针对 `Document.source` 的 UTF-8 源码块范围；保持 UI 文档版本 1，不授予图片、外链或编辑权限。宿主不解析 Markdown，也不执行 HTML；插件承担领域解析。原生通知保留面板和节点作用域。编辑区预览使用 `editor.documents` 与 `editor.read`，以带版本的内存文本通知驱动；旧画布/PTY 消息不会自动成为新基础协议的一部分。组合协议详见 [UI.md](UI.md)。

`Environment.locale` 显式传入用户界面语言，在 Prepare 与 `Notification::Theme` 中同步；缺失或空值保持简体中文默认。插件本身的提示和空状态应同时提供中英文，不读取项目设置来覆盖该用户选择。

`ui.images ^1` 用 `Kind::Image { source, alt }` / `Node::image` 声明源码版本绑定的原生图片，独立于 `ui.richtext`，保持 UI 文档版本 1。Document.source 必需，整篇最多 64 张、URI 最多 4096 字节；本地文档相对 URI 需要 `workspace.read`，HTTP(S) 需要 `network.images`。宿主后台加载只返回有限的原生资源快照，不向 WASM 输出大字节 JSON；缺权限、找不到文件、HTTP、解码、配额或超时错误仅影响对应图片。线程、字节限制、规范化工作区边界与取消生命周期见 [UI.md](UI.md#授权图片资源uiimages-10)。

`editor.presentation ^1` 允许工作区编辑区预览用 `Panel.view_modes` 声明源码、分栏、预览三个包内几何 SVG。该能力必须为 required，沿用 `editor.documents` 与 `editor.read` 门禁；不授予文件、网络或文本编辑权限。公开 `PreviewMode` 以 `source/split/preview` 序列化，默认 Split，供宿主按工作区保存原生布局选择。未声明模式的已有包无需增加能力或升级 `protocol=7` / UI 文档版本 1。图标检查与布局生命周期详见 [UI.md](UI.md#编辑区呈现模式editorpresentation-10)。独立 `editor_presentation` 实际包回归通过 `capability-example.zip` 重新打包为另一身份，验证协商、权限、资源边界与安装后的安全读取。

当前包安装和实例恢复要求 `protocol = 7` 及可协商的基础 API。协议 1–6 的已安装记录保留设置、权限、启用范围和私有数据，并显示需要更新；旧组件不会激活。使用当前 SDK 重建并安装同一插件的新包后恢复使用。旧运行协议与转换器已经删除；旧安装记录只参与管理界面展示和有限数据导入，不恢复执行。

独立验证插件为 `capability-example`，不属于正式发行包。先构建开发版宿主，再运行 `scripts/verify-plugin-sdk.ps1`：脚本将示例源文件、清单、README 和资源复制到系统临时目录，从该目录通过实际宿主的公开 `--plugin-cargo` 命令构建，不使用宿主仓库的 Cargo 工作区或业务源码路径。脚本还通过 `--export-plugin-sdk` 验证完整导出与损坏文件恢复。验证命令：

```powershell
cargo build -p editor-app
./scripts/verify-plugin-sdk.ps1 -HostExe ./target/debug/editor-app.exe
cargo test -p plugin-runtime --test sdk_distribution -- --ignored
cargo test -p plugin-runtime --test capability_packages
cargo test -p plugin-runtime --test capability_packages -- --ignored
cargo test -p editor-app --bin editor-app capability_package_consent -- --ignored
```

真实组件测试显式忽略默认运行，需要先生成测试包；不是跳过验收。脚本将同一 ZIP 输出至 `target/plugin-sdk-test/capability-example.zip` 与既有测试使用的 `target/plugin-api-test/capability-example.zip`，包内仅有清单、README、组件和声明资源。`sdk_distribution` 通过公开 `Package` / `Manager` 接口读取 README、安装真实组件并调用类型化错误诊断命令。日常快速构建仍可使用 `scripts/build-capability-example.ps1`，但仓库外分发验收以 `verify-plugin-sdk.ps1` 为准。

## workspace.files 1.1 与 host.sdk 1.0

`api::guest::find_files(&workspace, FileQuery { include, exclude, max_results })` 使用 `open_workspace` 返回的工作区根句柄，并在每次调用时重新检查 `workspace.files >=1.1` 与 `workspace.read`。私有数据、其他实例、已释放和已退役的句柄不能用于发现。应用作用域插件不拥有工作区根；调用不会跟随当前选中的其他工作区。

查询使用根相对、`/` 分隔的 glob；`**/` 可以匹配零级目录，`*` 不跨目录。绝对路径、驱动器前缀、反斜杠、空段、`.` 和 `..` 段无效。`include` 至少一项，include/exclude 合计最多 32 项，每项最多 1024 字节；`max_results` 为 1–4096。exclude 在目录入口剪枝，例如 `**/generated/**` 不会进入 generated 目录。宿主不内置语言项目或构建目录名称，由插件声明选择规则。

发现遵循根内 `.gitignore` 和 `.ignore` 的嵌套与否定规则，`.ignore` 优先；不读取工作区外的父级或全局 ignore，不跟随符号链接、Windows junction 或其他 reparse point。`FileMatches.paths` 是去重、排序的 UTF-8 工作区相对路径；`skipped` 是无法读取或解析的相对路径，调用者据此决定是否接受不完整发现。受限预算包括 50,000 个目录条目（含忽略项）、64 级目录深度、512 KiB 编码结果及最多 64 项 skipped；ignore 文件单个最多 64 KiB、合计 512 KiB/2048 行。超限返回 `LimitExceeded`，不会成功返回截断列表；遍历在文件系统调用之间检查当前插件调用期限，超时返回 `TimedOut`。

`api::guest::describe_sdk()` 需要协商 `host.sdk`，返回 `SdkDescriptor { digest, root, cargo_config }`，无需工作区读取权限。它描述宿主持有的同一份接口缓存：digest 是内容标识，root 与 cargo_config 是供原生语言工具使用的绝对路径。它不创建文件句柄、WASI preopen 或额外文件读取权限；将这些路径交给工作区 `read_file` 仍会被拒绝。宿主未提供 SDK 返回 `NotFound`，导出失败返回 `OperationFailed`，未协商返回 `CapabilityUnavailable`。

这两个操作可在活动实例及只读 `LanguageService` 准备钩子中调用。钩子可以关闭本次创建的文件句柄，其余临时句柄在返回时统一撤销；它不能通过发现发起写入、进程或编辑器操作。迁移钩子仍只有私有副本权限。宿主经 `Manager::open_with_resources` 提供不可变 `HostResources`，同一资源快照传入后台安装、设置替换、工作区切换及失败回滚后的实例。公开集成回归通过 `cargo test -p plugin-runtime --test sdk_discovery -- --ignored` 运行，先执行上面的独立 SDK 构建脚本。

## ui.clipboard 1.0 与 storage.editor 1.0

`EditorOperation::ReadClipboard` / `WriteClipboard { text }` 分别返回 `EditorValue::Clipboard { text }` / `Unit`。它们需要协商 `ui.clipboard` 并批准 `clipboard` 权限，单次文本不超过 1 MiB。操作通过编辑器请求队列执行，`Accepted` 只代表已入队；插件应等待对应请求的完成通知，不能将延迟结果应用到已经切换或关闭的目标。

`EditorOperation::OpenDataFile { path }` 需要协商 `storage.editor` 与 `storage` 权限。路径为当前插件私有目录内、使用 `/` 的相对文件路径，不允许 `..`、绝对路径或越界链接；成功返回 `Unit`，文件进入宿主正常文档生命周期。它不允许打开其他插件或任意本机文件。

这三种操作当前仅在活动工作区实例可用，遵循统一超时、取消与实例撤销规则。原生宿主在执行副作用前检查请求状态；取消不意味着回滚已经完成的剪贴板写入或文档打开。

## editor.edit 1.0 — 版本化选区与范围编辑

`EditorOperation::ReadDocumentSelection { document }` 要求 `editor.edit` 与 `editor.read`，返回 `EditorValue::DocumentSelection { document, range, text }`。它读取指定内存文档及 revision 的选区，不跟随工具栏获得焦点后其他编辑器的当前状态。`api::TextRange { start, end }` 为半开 UTF-8 字节范围，空范围表示光标；不能把字符数、UTF-16 单元或生成预览的偏移当作源码字节。

`ReplaceDocumentRange { document, range, text, selection, expected_selection }` 要求 `editor.edit` 与 `editor.write`。`range` 针对旧版本全文，替换文本最多 1 MiB；`selection` 针对替换后全文，定义结果选区。可选 `expected_selection` 针对旧版本，工具栏把读取时选区回传以拒绝选择竞态；无需该守卫的其他操作可留空。成功返回 `EditorValue::Edited { document, selection }`，其中 document 是事务后实际版本，不能提前返回旧 revision。

运行时拒绝倒置范围和超配额文本；原生宿主在执行前核对 workspace、文档身份、路径、revision、实际长度和 UTF-8 字符边界，并检查可选选区守卫。过期、已关闭或已重开目标不会改写当前焦点文档。所有写入通过唯一 DocumentSession/EditorState 提交为一个可撤销事务，不能建立另一份可变文本或撤销栈。取消、超时和实例退休复用编辑请求完成门禁：未进入副作用阶段的请求不可执行；进入事务后取消等待不承诺回滚已发生的修改，晚到完成结果不能覆盖终态。

两种操作只允许活动 workspace 实例，服务委派也分别核对来源 `editor.read` / `editor.write`。`editor.toolbar` 只提供源码顶部控件，布局、提示、预算与事件规则见 [UI.md](UI.md#源码工具栏editortoolbar-10)；执行格式动作必须另获上述编辑能力与权限。独立真实包回归为 runtime 的 `editor_edit` 集成测试，使用同一 SDK 示例重打包成不同插件身份。

## editor.images 1.0 — 原生图片输入与同级保存

`Document.editor_image_input: true` 为当前 `Document.source` 的源码编辑区声明图片输入，要求协商 `editor.images`，并具有 `editor.read` / `editor.write`；只允许自己的工作区 editor 面板。`ui.images` 负责显示图片，两项能力分别协商。此声明不允许访客轮询系统剪贴板或读取外部路径。

原生宿主捕获用户粘贴或拖入的真实图片，校验格式后调用 `Manager::offer_image_input`；剪贴板来源额外需要 `clipboard`，所有输入需要 `workspace.write`。宿主回调先核对活动预览、文档版本、选区和实例 epoch，再交付 `Notification::ImageInput { document, selection, images }`。`ImageInput { handle, format, byte_len }` 只有元数据；编码字节保留在 host 的 `Arc<Vec<u8>>` 资源，不穿过 JSON。`ImageFormat::extension()` 为 png / jpg / gif / webp / svg，后缀来自实际格式，不信任外部文件名。

每批最多 8 张，单张至多 8 MiB、每批至多 32 MiB，manager 待保存编码数据总计至多 64 MiB；原生入队也须预留有限内存。输入句柄在 30 秒后失效，精确归属于实例、面板、文档版本与半开 UTF-8 选区。预览撤销、源版本变化、文档关闭、工作区切换、插件停用或替换撤销尚未使用的输入。普通 `Manager::event` 不能伪造图片通知；文件或服务句柄不能代替图片句柄，服务调用不能借用这项原生授权。

访客通过 `EditorOperation::SaveImageInput { input, name }` 请求同级图片文件；需要 `editor.images`、`editor.read`、`editor.write`、`workspace.write`，请求期限最多 30 秒。`name` 为最多 255 字节的单一安全 basename，必须符合资源格式后缀；拒绝目录、绝对路径、Windows 设备名、ADS、控制字符、末尾空格或点。宿主重新核对目标工作区及规范化后的文档父目录，用原子 create-new 写入，不能覆盖旧文件；`Conflict` 保留句柄，访客可递增名称重试。同一输入不能同时受理两个保存。

宿主通过 `EditorRequest::image_input()` 获得不可变 `ImageInputResource { input, document, selection, bytes }`，完成返回 `EditorValue::ImageSaved { input, document, name }`，三个字段必须与受理请求一致；成功才消费输入。已受理的写入持有原始字节与目标，源预览变化不会丢失成功保存回执。取消的请求终态不会再变成成功：进入文件副作用后取消只能停止等待，不能声称撤销文件。因此访客应保留已受理保存任务以观察回执，源变化只撤销后续文本插入意图和未使用输入。

文件保存与引用编辑分别确认：本能力不编辑 Markdown 或其他文本，也不删除完整图片。访客在全部保存成功后使用 `editor.edit` 的一次版本化范围事务插入引用；迟到、关闭或选区变化的编辑会拒绝，成功文件保留并报告引用未插入。文本 Undo 只撤销引用。创建失败时，宿主仅清理该请求创建的不完整新文件。`workspace.write` 在这里授权目标文档同级附件，既有 `workspace.files` 仍为只读，不获得任意工作区写入能力。

公开独立回归：先构建 capability-example，再运行 `cargo test -p plugin-runtime --test editor_images -- --ignored --test-threads=1`；原生剪贴板、拖入、实际文件和 Undo 由宿主集成验收。

## configuration 1.0

协议 7 清单的 `settings` 以插件内部键声明 `title`、`value_type`、`default`、`scope` 和 `apply`。类型为 boolean、string（max_length）、integer（min/max）、enum（choices）；作用域为 user 或 project，后者允许明确确认的项目覆盖。此版本的生效方式为 `restart_instance`。最多 64 个字段、字符串最多 4096 字节、枚举最多 32 个不重复选项；无效默认值在包检查时拒绝。可执行包需声明必需能力 `configuration: ^1`。

可选 `settings_hook: true` 接收 `Notification::Configuration { phase: Validate, values }`，通过 `Output.configuration: Proposal` 返回 discovered 和 errors。钩子在候选实例的准备阶段运行，继承有限指令与内存预算，不能获取活动实例资源或授权；读取已获权限的包资源仍可用。发现值只填充未显式设置的项，任何错误使应用失败。随后 Apply 阶段收到带 Source 的最终值，供 Activate 使用；此阶段失败也不替换旧实例。配置被限制在插件命名空间，访客不能经配置获得权限或修改宿主用户设置。

用户设置独立于访客私有文件，已确认项目值保存在宿主管理的工作区记录，不自动信任仓库文件。热应用、初始化与再次打开工作区使用相同解析路径。应用失败保留先前配置和运行实例；成功只替换相关实例并撤销其旧资源。

## 独立构建与 SDK 分发

本目录是主程序持有的版本化插件接口。`wit/plugin.wit` 定义 WebAssembly Component Model 的导入与导出；Rust crate `plugin-protocol` 定义通过该接口传递的 JSON 消息、文档和权限名称。主程序把这些接口文件编入可执行文件，并自动管理插件编译所需的接口缓存。

`host.request` 是主程序在运行时提供的导入。当前插件通过 `api::guest` 使用类型化能力请求，主程序逐次检查协商能力、插件权限、实例作用域和句柄。WIT 和 Rust 类型只在编译插件时使用，安装后的 `.wasm` 不读取 SDK 文件。

插件在 `Cargo.toml` 声明 `plugin-protocol = { version = "=0.2.0", features = ["guest"] }`，通过 `plugin_protocol::bindings::{Guest, editor, export}` 使用宿主调用和组件导出，无需自行生成 WIT 绑定。独立编译时直接调用已打包的编辑器：

```powershell
editor-app.exe --plugin-cargo capability-example/Cargo.toml build --target wasm32-wasip2 --release
# 同一入口也支持原生单元测试和编译检查。
editor-app.exe --plugin-cargo capability-example/Cargo.toml test --lib
editor-app.exe --plugin-cargo capability-example/Cargo.toml check --target wasm32-wasip2
```

编辑器将内嵌接口按内容摘要缓存到系统用户缓存目录的 `MeEditor/plugin-sdk/<摘要>/`（Windows 为 `%LOCALAPPDATA%/MeEditor/plugin-sdk/<摘要>/`），通过本次 Cargo 命令的依赖覆盖选择该缓存。不同接口版本不会互相覆盖；缓存缺失或损坏会自动恢复。插件项目不需要 `sdk/`，也不引用主程序源码；发行目录只需主程序与插件包。接口变更时仍需核对 WIT 包版本和插件清单的 `protocol` 版本。

开发机器需安装 Rust/Cargo 与 `wasm32-wasip2` 目标；使用已编译插件的用户无需这些工具。`--plugin-cargo` 执行开发者提供的 Cargo 项目，属于本机开发工具，不是运行时沙箱。为其他语言工具链或接口检查保留显式 `--export-plugin-sdk <目录>`，常规构建与发行不调用它。

在本编辑器内开发插件时，Rust 插件通过 `host.sdk` 获取与 `--plugin-cargo` 相同的宿主 SDK 缓存，通过 `workspace.files` 发现独立插件项目。插件的 WASM 钩子生成 Rust Analyzer 的 `cargo.configPath`、`linkedProjects` 和配置节；宿主按通用 LSP 协议传递这些数据。项目发现遵循忽略规则，并由 Rust 插件声明排除构建产物和 vendor 目录。输入 `plugin_protocol::api::` 可以获取类型补全、悬浮说明和定义跳转。插件目录无需增加 SDK、Cargo 配置或指向宿主源码的路径；定义跳转会打开宿主缓存中的协议源码。需要已启用 Rust 语言插件并安装 Rust Analyzer。

## process 1.1 与 language.lsp 1.1

原生服务可声明 `search_paths` 与 `check_args`。前者最多 32 个绝对路径 glob，唯一支持的变量为 `${HOME}/` 前缀；不得包含 `..`、控制字符或递归 `**`。按声明顺序搜索，同一模式的候选按路径倒序排列，然后查找绝对 PATH 目录。搜索最多访问 20,000 个目录条目，中间候选和发现结果各最多 256 个，整个发现最多五秒；访客请求同时受本次调用的更短期限约束。`check_args` 作为字面参数数组运行，不使用 Shell；探测最多两秒并共享发现剩余期限，退出失败的候选被跳过。所有探测使用宿主进程所有权和清理规则。

服务 `program` 或 `executable_setting` 的显式绝对路径只有一个候选，错误会直接呈现，不回退到搜索结果。安装服务权限批准该声明的查找和探测行为；这些字段不授予动态可执行程序权限。

LSP 提供者可声明 `client_experimental`，传入 `initialize.capabilities.experimental`；标准传输能力仍由宿主维护。最多 64 个非空键，每键最多 256 字节，JSON 深度最多 16，且包含在提供者总计 256 KiB 的限制内。宿主不识别具体服务名称、扩展字段或 Rust 项目结构。

## 当前版本与迁移边界

Rust SDK 0.2.0 删除旧 Message/Event/Reply/Request、Scene/Widget 与 CanvasControls 类型和兼容转换。重新编译时依赖 `plugin-protocol = { version = "=0.2.0", features = ["guest"] }`。当前能力协议的线上格式不变：清单 protocol 7、api.base ^1；WIT 仍是 editor:plugin/plugin@0.1.0，接口能力分别协商版本。

插件只使用 api::Input、api::Notification、api::Output 和 api::Operation；UI 返回 ui::Document，其中 Canvas、SideTabs 是普通节点。宿主嵌入端通过 Manager::event(plugin_id, panel, notification) 路由，读取 Instance::views 获取每个面板的已验证文档，不再经过旧场景转换。

旧协议 1–6 的包拒绝安装和执行，保留安装记录以展示更新/卸载与启用偏好。仅保留有限的历史 ID、私有数据与布局导入；数据导入不会恢复旧代码执行能力。热更新和迁移详情见 [MIGRATION.md](MIGRATION.md)。
