# XML 语言工单 01 验收记录

日期：2026-10-07。对应 [#73](https://github.com/T-miracle/Editor/issues/73)、[工单](../../tickets/xml-language-tools/01-xml-language.md)与[方案](../../specs/xml-language-tools.md)。

状态：已交付。实现提交 `400c5310ca0dd9995b66f23ea1d440c24a89c5f3` 已推送，`git ls-remote` 核对远端分支 SHA 一致；2026-10-07T10:08:45Z 关闭 #73 后已读回 `closed/completed`。窗口截图限制见下文，不宣称已取得可用截图。

## 验证环境与边界

- Windows x86_64 MSVC；独立工作树 `codex/xml-language-tools`，实现基线 `ae63225`，规划提交 `09f31122`。
- 最终宿主、测试与原生例程使用本工作树独立 `target/native/`。初期复用原项目 `target/`，后发现其产物被另一工作树覆盖（dep-info 指向 `Editor-shortcuts-main`，缺少本单契约）；只复制第三方依赖缓存，排除本 workspace crate 并重新构建。未停止其他任务或继续清理共享缓存。
- 实际 XML 和 Image ZIP 经正式 `--plugin-cargo` SDK 入口构建；最终 SDK 由独立目录重新构建的宿主导出，未以旧宿主证明新契约。
- 测试通过实际包、公开 Manager 安装、既有 worker 发布及 GPUI 编辑会话，未新增 XML 专用测试 API。
- 原工作区的既有代码改动没有复制到工作树或计作本次成果。

## 已执行的针对性验证

| 验证 | 实际命令 | 结果 |
| --- | --- | --- |
| 文件关联设置、已打开文档与停用/启用 | `cargo test -p editor-app custom_extension_association_updates_open_document --no-default-features` | 1 passed，0 failed；通过真实设置窗口输入 `.CFG` 并选择陌生语言提供者，文档即时更新；非法路径拒绝，提供者停用后恢复纯文本 |
| XML 与 Image 组合 T21 | `cargo test -p editor-app xml_and_image_packages_share_unsaved_svg_and_independent_lifecycles --no-default-features -- --ignored` | 1 passed，0 failed；真实 `xml.zip` 与 `svg.zip` 独立启停/卸载，共用同一 EditorState；未保存中文/emoji SVG 到达预览并生成栅格，磁盘不变，深浅主题可见源码及预览 |
| 原生 XML 补全输入 | `cargo test -p editor-app installed_xml_completion_applies_through_native_input --no-default-features -- --ignored` | 最终独立目录 1 passed，0 failed，81.46 s；新契约下真实包与服务经过原生中文输入、emoji 后的字节位置、补全菜单、主题切换和 Enter 应用，磁盘不变 |
| 无版本诊断的寿命归属 | `cargo test -p editor-app installed_snapshot_service_rejects_late_unversioned_diagnostics --no-default-features -- --ignored` | 1 passed，0 failed；实际陌生包和 stdio 服务证明修改、关闭重开、迟到 close、未证明的规范 URI 推送及停用均不能覆盖当前结果 |
| 工作区外原生补全回归 | `cargo test -p editor-app native_completion_remains_available_outside_guest_workspace --no-default-features -- --ignored` | RED：真实菜单丢失，1 failed；修复后 GREEN：1 passed，0 failed，22.11 s。原生实体/revision/文本照常校验，工作区外文件仍被插件文档接口拒绝；保留原生补全及 Enter 应用 |
| 真实 XML 语言矩阵 T01–T06 | `cargo test -p editor-app xml_plugin_package_enters_existing_language_registry --no-default-features -- --ignored` | 1 passed，0 failed，462.99 s；五种格式真实 grammar capture、基础/Unicode EOF 补全、XSD/DTD/SVG 约束、语法与命名空间诊断、悬浮和精确 Schema 定义；无 Schema 无告警，缺失/禁网/503 同文件保留基础属性补全，获取后缓存且断网可用 |
| Schema 路径与关联回归 | 同一真实矩阵 | 原生根及规则文件含 `#`、`%`，`../` Schema/Catalog 和 Catalog 相对 DTD；绝对关联、嵌套目录 suffix、无关关联/Catalog、非 xsi 命名空间、Schema 恢复成功后拒绝非法历史属性均通过 |
| 依赖首次降级与更新保留 | `cargo test -p plugin-runtime --test optional_language_dependencies` | 1 passed，0 failed，0.11 s；使用公开包管理入口证明首次准备失败保留资源，更新准备失败保留旧版本，取消不会被降级吞掉 |
| 纯补全真实 WASM 与契约 | `cargo test -p plugin-runtime --test pure_language_completions -- --ignored` | 1 passed，0 failed，121.35 s；实际仓库外 SDK ZIP 验证能力/权限、拒绝 IO、UTF-8 范围/nonce/revision、过期与超量输出、逻辑 URI/有界诊断摘要、设置变更撤销旧实例和停用清理 |
| 纯钩子故障不破坏原生服务 | `cargo test -p editor-app pure_completion_faults_preserve_native_results_and_process --no-default-features -- --ignored` | 1 passed，0 failed，42.22 s；迟到、坏范围、超量及 fuel 错误保留原生候选、可见错误日志和唯一存活服务，后续健康补充及原生请求继续正常 |
| SVG 合法展示值回归 | `cargo test -p editor-app xml_svg_assistance_preserves_valid_presentation_values --no-default-features -- --ignored --test-threads=1` | 真实服务 RED：四项合法 `inherit` 值产生 8 条枚举/属性误报；放宽辅助规则后正式包 GREEN：1 passed，0 failed，89.46 s。属性名提示和语法错误诊断保留；用户 XSD/DTD 枚举机制不变 |
| Windows 深目录依赖收据 | `cargo test -p plugin-runtime --test optional_language_dependencies long_private_installation_root_publishes_optional_dependency_receipt -- --exact`；修复后运行该测试文件 | 公开陌生 ZIP 安装/重开 RED：1 failed、os error 3；规范化父目录后 GREEN：该文件 2 passed，0 failed，0.04 s。原子替换与可选依赖降级保持一致 |
| 最终格式检查 | `cargo fmt --check`；对本单全部 Rust 文件追加 `rustfmt --check --edition 2024 --config skip_children=true` | 均通过，包含独立插件源 |
| 最终非 UI 检查 | `cargo test --workspace --exclude editor-app` | 所有资源/长路径修复后独立 native 目录：198 passed，0 failed，168 ignored；本单所需实际包/纯钩子测试另显式执行，没有将 ignored 计为通过 |
| 最终 workspace 编译 | `cargo check --workspace` | 独立 native 目录通过；已有未使用/死代码及 MSVC Tree-sitter/Wasmtime 警告未扩散修复 |
| 当前 SDK 独立分发 | `./scripts/verify-plugin-sdk.ps1 -HostExe <工作树 target/native/debug/editor-app.exe>` | 最终独立宿主通过 SDK 导出、缓存修复和仓库外独立 WASM 构建（guest 25.85 s）；输出夹具供公开 Manager 回归使用 |
| 双语读者文档 | `npm test`（`website/`） | 15 passed，0 failed；2 个搜索索引测试因没有站点构建产物跳过，未宣称其通过；本单未改搜索或版式 |
| 差异空白 | `git diff --check` | 通过；新文件在最终暂存后补查 |
| 原生 XML 与 Image 组合启动 | `./scripts/svg-startup-smoke.ps1 -HostExe target/native/debug/editor-app.exe -CompanionPackages @('dist/plugins/xml.zip')` | 正式 ZIP 经公开安装器进入真实 Windows 窗口，启动、预览表面生命周期和正常关闭/私有快照保存通过；隐藏 GPU 窗口 `capture=False`，未取得可用 PNG。输入、布局、主题与未保存栅格结果由上述 GPUI 原生控件验证证明，不把此项称为截图验收 |

T21 初次编译遇到 Windows `LNK1104`：另一个正在运行的测试占用同一 EXE。串行安排应用测试后同一命令通过；不是产品缺陷。

非 UI 检查首轮链接器找不到 `msvcrt.lib`，已确认库存在而当前进程未载入搜索路径。仅在命令进程设置 `LIB` 为已安装 MSVC 14.50.35717 的 `lib/x64` 和 Windows Kit 10.0.26100.0 的 `ucrt/x64`、`um/x64` 后通过；没有修改全局环境或安装工具。

长路径修复后重新链接例程时再次遇到同库搜索问题；使用对应目录的原生反斜线 `LIB` 值后，`terminal_resize` 构建与最终完整非 UI 检查均通过。窗口夹具第一次安装 XML 失败的 backtrace 定位到依赖收据写入，随上述通用长路径回归修复后同一烟测通过；没有通过缩短路径隐藏缺陷。

## Standards

初审固定比较：`git diff ae63225...50d25aae7c68a35b73e78a275b9a461138990145`，发现 1 项 P3：中文把 “Schema-bound file” 译成“Schema 文件”。候选 `add630ebfa9fb2a07d46685b0bc682aab1c6b915` 已改为“已关联 Schema 的文件”，独立补审确认解决，新硬性违规及可行动坏味道均为 0。最终候选 `67c218f803c310b085c066bf6855b378f250aea1` 的父路径两文件增量再次独立补审：两类发现均为 0。

最终交付源码候选 `fdf60ddabb2f047cd0e8d98beb98494f7b801ac0` 的 SVG 资源及长路径四文件增量补审：新硬性规则违反 0，可行动坏味道 0。

## Spec

同一初审候选发现 2 项 P2：任意 Catalog/绝对关联及失败 Schema 被过早视作有效约束，关闭历史属性建议；本地路径直接拼 URI，导致合法 `#`、`%` 名称被错误解释。候选 `add630e` 的独立补审确认两项已解决，又发现 1 项新 P2：URI 库忽略 `..`。最终候选 `67c218f` 在编码前归并原生父路径，并补真实 Schema/Catalog 回归；独立补审确认该项解决，新增可行动发现及范围蔓延均为 0。

候选 `fdf60dd` 最终小增量补审：新增可行动发现 0，未发现缺失、错误实现或范围蔓延。作者/主代理自查的 SVG 误报及 Windows 深目录写入各自保留 RED/GREEN 证据，不改写审查轴的原始发现。

两轴分别保留原始发现：Standards 初审 1 项（P3），全部解决；Spec 初审 2 项（P2）及增量 1 项（P2），全部解决。最终两轴各 0 个未解决发现；审查代理未修改代码或执行 Cargo。工作区外原生补全是主代理自查补充的回归修复，不合并或改写两轴原始发现。

## 包与分发身份

- XML 清单/guest：`0.1.0`；协议 7；`language.lsp >=1.3,<2`、`language.completion ^1`、`dependencies >=1.1,<2`。最终 SVG 资源修复后 ZIP SHA-256：`203f685a6db17f3e7766b89a9657a5ffe5c25d56ce31a5f3263cc5483993b806`。
- Capability 示例夹具：`0.16.2`，最终独立 SDK ZIP SHA-256：`33eabda6b22ecd79279d9f1a37a0e66371c4da2f77e1a3211e3458747de12646`。
- Tree-sitter XML `0.7.0`、ABI 14、MIT；grammar SHA-256：`9950327f8542a3d563fe4192aa05986a53e6fd6adca518cbb1a1699cbc8970ad`。来源、许可证与重建方式见 [grammar 说明](../../../../plugins/xml/grammar/README.md)。
- Windows x86_64 LemMinX `0.31.2` 固定 native 分发来源和下载 hash 见 [插件说明](../../../../plugins/xml/README.md)。其他平台支持显式已有程序配置，但本记录不宣称已做跨平台执行验收。
- 最终 Windows 启动验收宿主 SHA-256：`763c3dab09695b07431eb3b545f62ae3112c7ac16a7df273b60031c511952a5f`。长路径修复没有改变协议/SDK 源，复用此前独立 SDK 契约验收，不重复无关构建矩阵。

## 交付记录

- 普通提交：`400c5310ca0dd9995b66f23ea1d440c24a89c5f3`，已推送 `origin/codex/xml-language-tools` 并核对远端 SHA。
- #73 已关闭并读回 `state=closed`、`state_reason=completed`。02 与 03 因此解除阻塞；本记录只标记 01 完成，父 #72 未修改。

主代理的设置、T21、原生输入、阶段检查、长路径与窗口日志位于 `target/xml-language-tools/`；XML 实施代理的服务矩阵、纯钩子、SVG 资源及独立 SDK 日志位于 `target/xml-verification/`。本单实际结果和关闭状态已更新。
