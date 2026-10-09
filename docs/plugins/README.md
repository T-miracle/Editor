# 插件系统文档目录

本目录索引实际维护的插件资料。跨插件平台方案放在本目录的 specs、tickets 与 verification；插件自身的历史专题资料保留在对应包内，不复制正文或继承其他任务的完成状态。

- [插件打包与隔离开发运行](specs/plugin-development-packaging.md)、[本次验收](verification/plugin-development-packaging.md)：宿主模板与 CLI 共享项目描述，自动 ZIP、输出选择和无 ZIP 的独立开发运行。
- [插件开发工作区整合主工作区改动](verification/plugin-development-main-integration.md)：2026-10-08 用户授权整合全部未提交改动，包含两侧备份、冲突处理与重新验证；未提交或推送。
- [缺失项目描述修复与主工作区 Release 交付](verification/plugin-development-main-delivery.md)：截图错误的回归测试、主工作区整合与 Release 启动证据。

- [基础社区生态：通用插件 API 完善方案](specs/plugin-api-community-foundation.md) / [#92](https://github.com/T-miracle/Nanobug/issues/92)、[七张实施工单 #93–#99 与测试安排](tickets/plugin-api-community-foundation/README.md)、[实施与验收入口](verification/plugin-api-community-foundation/README.md)：01、02 已验收、推送并读回关闭，06 实施中；其余按依赖继续。稳定核心＋实验扩展，AI 暂不实现，整批尚未完成。
- [01 / 02 普通合并连接验证](verification/plugin-api-community-foundation/01-02-integration.md)：文档资源与原生命令交互的本地候选；新增菜单与焦点连接的审查、原生复核待完成。
- [通用插件接口完整性审计](specs/plugin-api-ecosystem-audit/README.md)：2026-10-08 的 58 项历史调查；语言接口已随后整合，当前实施范围见基础社区生态方案，不等于所有检查项获准实施。
- [XML 插件与通用语言编辑能力方案](specs/xml-language-tools.md) / [#72](https://github.com/T-miracle/Editor/issues/72)，[三个实施工单 #73–#75](tickets/xml-language-tools/README.md)：全部验收、推送并核对关闭；包含可替换格式化、XML/HTML 标签编辑及宿主大纲与停靠，父设计议题保持不变。
- [XML 语言工单 01 验收记录](verification/xml-language-tools/01-xml-language.md)：记录实际包、原生输入与服务结果，当前状态以记录为准。
- [格式化与标签编辑工单 02 验收记录](verification/xml-language-tools/02-format-and-tags.md)：独立提供者、XML/HTML 配对编辑及原生输入顺序。
- [宿主大纲工单 03 验收记录](verification/xml-language-tools/03-outline-and-docking.md)：结构与图标契约、原生树、折叠及四向停靠恢复。
- [02/03 共同交付记录](verification/xml-language-tools/02-03-delivery.md)：一次共同检查、SVG/XML 短组合、双轴审查及准确议题交付状态。
- [大纲点击崩溃修复](verification/xml-language-tools/04-outline-click-crash.md)：2026-10-08 用户窗口回归、标题栏实体借用修复及真实点击验证。
- [解析取消后的原生撤销回归](verification/xml-language-tools/05-parser-cancellation.md)：实际 XML 组合复验捕获的独立解析器生命周期缺陷与恢复验证。
- [大纲单一高亮与临时展开](verification/xml-language-tools/06-cursor-highlight-and-expansion.md)：光标位置唯一选中背景、默认两层及移动后收起旧自动路径。
- [Windows 插件状态原子替换恢复](verification/plugin-state-atomic-replacement.md)：启动写入短暂占用回归、有限重试、原数据保护和持续失败定位。
- [XML 分支合并与工作区推送](verification/xml-language-tools/07-main-merge-and-publication.md)：各工作区分支发布、XML 先推送再合并，以及宿主打包与快捷键的集成验证。

- [插件安装进度弹窗尺寸调整](verification/plugin-install-progress-size.md)：紧凑窗口、长进度信息滚动与底部按钮布局回归。
- [插件 UI 解耦与文件显示布局总方案](specs/plugin-ui-decoupling.md)、[五个实施工单（#62–#66）](tickets/plugin-ui-decoupling/README.md)。已在 `codex/plugin-ui-decoupling` 完成实现、验收及双轴审查，五张工单均已推送并核对关闭。
- [工单 01：Image 与文件显示验收](verification/plugin-ui-decoupling/01-image-file-views.md)。
- [Image 窗口尺寸回放修复](verification/image-resize-verification.md)：连续画布尺寸合并，保留滚轮输入与居中。
- [Image 普通图片滚轮缩放与 SVG 240px 最小初始尺寸](verification/image-zoom-verification.md)：Image 0.5.1 与通用原生图片控件验收。
- [通用可视视口能力](specs/visual-viewport.md)、[Image 0.6.0 验收](verification/image-viewport-verification.md)：缩放策略移回插件，文件图片、资源图片与画布共用公开能力。
- [工单 02：可组合布局与提供者选择验收](verification/plugin-ui-decoupling/02-composable-layouts.md)。
- [工单 03：两组底栏与共享偏好验收](verification/plugin-ui-decoupling/03-tools-and-preferences.md)。
- [工单 04：真实插件与历史显示偏好迁移验收](verification/plugin-ui-decoupling/04-plugin-migration.md)。
- [工单 05：最终契约与集成验收](verification/plugin-ui-decoupling/05-integration-and-contract.md)。
- [本地 main 合并与原有改动整合](verification/plugin-ui-decoupling/06-local-main-merge.md)：合并提交 `db53d2d`，保留合并前未提交内容与备份，按用户要求暂不推送；当前工作区的验证结果与环境限制见该记录。

- [Markdown 使用说明](../../plugins/markdown/README.md)、[总方案](../../plugins/markdown/docs/spec.md)、[执行工单](../../plugins/markdown/docs/tickets/README.md)、[逐单验收](../../plugins/markdown/docs/verification/README.md)、[AI 执行入口](../../plugins/markdown/AGENTS.md)。
- [插件平台规格](specs/plugin-api-platform.md)、[平台实施工单](tickets/README.md)、[运行与构建](runtime-plugins.md)、[公开协议与 SDK](../../crates/plugin-protocol/README.md)。
- [插件管理与日志方案](specs/plugin-management-logs.md)、[对应工单](tickets/plugin-management-logs/README.md)、[管理页验收](verification/plugin-management-tabs-verification.md)。
- [运行、调试与构建父议题](https://github.com/T-miracle/Editor/issues/48)、[接手终验与工单处置](verification/run-debug-build-completion-2026-10-06.md)、[B1 UI 修复与复验](verification/run-debug-build-ui-fix-2026-10-06.md)、[历史验收](verification/run-debug-build.md)、[整批初审记录（2026-10-05）](verification/run-debug-build-review-2026-10-05.md)。当前进度以终验及复验记录为准。
- [本批调试器、受控传输与单目标握手决定](specs/run-debug-build-debugger.md)。
- [构建与运行配置 B2：IDEA 参考设计提案](specs/run-config-idea-design.md)（用户已选 A / 方案 1）；[B3：双栏简洁版](specs/run-config-simple-design.md)（已落地原生弹窗，支持插件默认目标）；[B3 实现与验收](verification/run-config-simple-2026-10-06.md)。设计图与真实验证的范围在对应记录中区分。
- [运行配置重构：插件模板、原生表单与配置树](specs/run-config-plugin-tree.md)／[规格 #67](https://github.com/T-miracle/Editor/issues/67)（2026-10-07 产品基线及测试入口已确认）。新规格替代 B3 的表单、目录组织、提交和存储约定，不将旧验收计作新功能通过。
- [运行配置重构实施工单](tickets/run-config-plugin-tree/README.md)：4 个端到端切片 #68–#71 的实现与验收均完成，普通提交及 GitHub 交付记录见各工单。30 个验收场景的实际包、Windows 与真实调试证据见 [验收记录](verification/run-config-plugin-tree.md)。2026-10-07 用户另行授权合并主分支并推送，独立候选的冲突处理和验证见 [主分支集成记录](verification/run-config-main-merge-2026-10-07.md)。

Windows 原生验收已通过；macOS/Linux 未实测或交叉构建。七个正式插件包已在本地构建和验收，未发布 GitHub Release。完整结果以[最终契约验收](verification/plugin-api-contract-verification.md)为准。

## 方案、工单与使用文档

新增方案：[插件 UI 解耦与文件显示布局总方案](specs/plugin-ui-decoupling.md)，父议题 [#61](https://github.com/T-miracle/Editor/issues/61) 保持 open；[5 张粗粒度工单 #62–#66](tickets/plugin-ui-decoupling/README.md) 已完成并关闭，[发布记录](tickets/plugin-ui-decoupling/publication.json)保存映射。保留仅列文件的 Tab 栏，插件安排中心布局并可省略原生编辑面板；左侧窗口按钮组与右侧插件工具按钮组分离，图标、功能和显示偏好由插件提供，同工作区同类文件一起切换。[工单 01 的 Image 插件升级](tickets/plugin-ui-decoupling/01-file-views.md)已实施：由现有 SVG 扩展，SVG 保留编辑，其他支持图片只读预览，默认原尺寸、超区等比缩小。分支交付证据与本地 main 合并验证分别记录，不把原工作区的未提交内容计作分支成果。

新增方案：[运行、调试与构建总方案](specs/run-debug-build.md)，已发布为 [#48](https://github.com/T-miracle/Editor/issues/48)，对应 [12 张工单 #49–#60](tickets/run-debug-build/README.md)（产品范围、B1 紧凑表单、拆分与测试接缝均已确认，待实施；16 条原生阻塞边已核对）。覆盖顶栏统一配置／会话入口、独立配置并行、顺序启动前步骤、Rust 完整启动调试及公开执行／调试接口。

Markdown 插件：[总方案](../../plugins/markdown/docs/spec.md)、[工单目录](../../plugins/markdown/docs/tickets/README.md)、[AI 执行入口](../../plugins/markdown/AGENTS.md)。完整宿主能力与 Markdown 0.11.1 已合回主项目 `main`；SVG 0.3.0 同样采用底栏三个视图按钮。主目录构建、原生回归及发行文件见[主项目集成验收](../../plugins/markdown/docs/verification/12-main-project-integration.md)。原 0.11.0 工单及完整验收保留历史证据，早期[仅迁入源码的记录](verification/markdown-source-repackage.md)不代表后续宿主集成状态。

后续缺陷：[源码区只有一行的布局修复](../../plugins/markdown/docs/verification/13-source-editor-height.md)，直接验证工具栏下方的实际输入高度、空文档、SVG、主题与窗口重排。

后续更新：[Markdown 0.12.0 图标工具栏与首次滚动修复](../../plugins/markdown/docs/verification/14-icon-toolbar-and-first-scroll.md)，采用统一 SVG 与 29 px 单行工具栏，底栏面板图标移至同步开关右侧，验证同版本启动刷新保留手动滚动位置。

当前后续范围：[Markdown 0.13.0 GitHub 配色、六级标题、工具栏显隐与小幅滚动](../../plugins/markdown/docs/verification/15-github-headings-toolbar-scroll.md)。底栏图标改为仅控制顶部工具栏，旧记录中面板开关语义保留为历史说明。

已完成批次：[插件管理与运行日志改造方案](specs/plugin-management-logs.md)（[#22](https://github.com/T-miracle/Editor/issues/22) 已于 2026-10-05 核对完成并关闭），对应[3 张实施工单](tickets/plugin-management-logs/README.md)，#23–#25 均已完成并关闭。验收见[分栏验证](verification/plugin-management-tabs-verification.md)、[运行日志验证](verification/plugin-runtime-logs-verification.md)和[底栏摘要验证](verification/plugin-status-popover-verification.md)；后续本地日期与日志降序修正也已通过[补充验收](verification/plugin-log-time-order-verification.md)。本批次独立于下表已完成的平台重构。

启动修正：[Windows 插件管理器访问拒绝验收](verification/plugin-manager-access-verification.md)（2026-10-04 已完成）。确认仓库继承的 Low 完整性标签导致 `os error 5`，本机已恢复正常目录标签并移除临时启动包装；直接 `cargo run` 与 `cargo run --release` 均通过原生验收，安装状态保持完整。

后续复发：[默认构建目录访问拒绝复发验收](verification/plugin-manager-access-recurrence.md)（2026-10-06）。再次确认原仓库根和 EXE 被标为 Low，恢复根目录及默认构建输出的 Medium 继承；新 Cargo 输出和原目录 release 插件管理器均通过，5 条安装记录保留。标签外部来源尚未确定，历史已修复状态不能替代当前文件标签检查。

| 分类 | 入口 | 状态 |
| --- | --- | --- |
| 平台设计 | [插件平台方案](specs/plugin-api-platform.md) | 已完成实施，正文保留设计基线 |
| 实施工单 | [20 张工单、议题映射及依赖图](tickets/README.md) | 全部已完成 |
| 最终验收 | [T01–T26 契约验收](verification/plugin-api-contract-verification.md) | 已完成，含真实执行证据与限制 |
| 运行与构建 | [运行时插件平台](runtime-plugins.md) | 当前使用参考，随平台维护 |
| 声明式主题 | [主题插件格式](主题插件格式.md) | 格式参考，随契约维护 |
| 公开 SDK | [协议与 SDK 文档](../../crates/plugin-protocol/README.md) | 当前契约；专题文档保留在源码旁 |

## 分阶段验收记录

以下记录均对应已完成工单。正文保留执行当时的范围、结果和限制；早期“后续工单”“旧协议暂存”等描述是历史阶段说明，最终行为以完整契约验收和当前 SDK 为准。

- [bootstrap](verification/plugin-api-bootstrap-verification.md)
- [composable-ui](verification/plugin-api-composable-ui-verification.md)
- [contract](verification/plugin-api-contract-verification.md)
- [data-migration](verification/plugin-api-data-migration-verification.md)
- [dependencies](verification/plugin-api-dependencies-verification.md)
- [execution-service](verification/plugin-api-execution-service-verification.md)
- [hot-update](verification/plugin-api-hot-update-verification.md)
- [installed-upgrade](verification/plugin-api-installed-upgrade-verification.md)
- [installers](verification/plugin-api-installers-verification.md)
- [language-migration](verification/plugin-api-language-migration-verification.md)
- [language](verification/plugin-api-language-verification.md)
- [lsp](verification/plugin-api-lsp-verification.md)
- [process](verification/plugin-api-process-verification.md)
- [recovery](verification/plugin-api-recovery-verification.md)
- [requests](verification/plugin-api-requests-verification.md)
- [scopes](verification/plugin-api-scopes-verification.md)
- [services](verification/plugin-api-services-verification.md)
- [settings](verification/plugin-api-settings-verification.md)
- [terminal-migration](verification/plugin-api-terminal-migration-verification.md)
- [ui-migration](verification/plugin-api-ui-migration-verification.md)

## 插件包说明

- [终端](../../plugins/terminal/README.md)
- [示例](../../plugins/example/README.md)
- [SVG](../../plugins/svg/README.md)
- [Rust](../../plugins/rust/README.md)
- [TOML](../../plugins/toml/README.md)
- [HTML](../../plugins/html/README.md)
- [JavaScript](../../plugins/javascript/README.md)
- [XML](../../plugins/xml/README.md)
- [Markdown](../../plugins/markdown/README.md)
- [Markdown 输入与增量预览验收](../../plugins/markdown/docs/verification/17-incremental-input-viewport.md)（0.15.0：用户实测输入仍卡顿，性能验收重新打开）
- [Markdown 源码输入与过期视口通知](../../plugins/markdown/docs/verification/18-source-input-and-stale-viewport.md)（宿主跟进修复与实测限制）
- [能力协议验收夹具](../../plugins/capability-example/README.md)：仅用于验收，不属于发行插件目录。

## 后续管理约定

插件系统的新方案、扩展、迁移和缺陷工单均归入本目录：`specs/` 保存方案，`tickets/` 保存工单与议题映射，`verification/` 保存验收。使用能区分主题或批次的文件名或子目录，保留本轮 01–20 的历史编号，并更新本索引。

每份方案和工单标明实际状态、对应规格、相关工单和验收入口；完成后同步状态与验收项。新任务不继承本轮“已完成”状态或提交、推送、关闭授权。历史记录不覆盖最终完成状态，也不把未验证的平台或未发布的资产标成完成。

返回[文档总目录](../README.md)。

已补齐 `specs/`、`tickets/`、`verification/` 中的平台历史资料；原 `docs/specs/` 保留旧路径记录，不以旧副本覆盖现行管理与日志文档。内容不同的早期资料见[2026-10-03 历史入口](archive/2026-10-03/README.md)。
