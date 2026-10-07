# XML 语言工具 02/03 共同交付记录

日期：2026-10-07–08。范围为 [02 / #74](../../tickets/xml-language-tools/02-format-and-tags.md)、[03 / #75](../../tickets/xml-language-tools/03-outline-and-docking.md)。详细行为结果分别见 [02](02-format-and-tags.md)、[03](03-outline-and-docking.md)；01 的既有验收见 [01](01-xml-language.md)。

状态：最终共同 Rust 门禁、独立 SDK 分发、正式语言包、SVG 短组合及当前生产窗口启动均通过；三项审查 P2 与一项判断性 P3 均修复，两个最终审查轴发现为 0。验收完成，待普通提交、推送及工单关闭读回。

## 固定范围与减少重复

- 隔离工作树：`C:\Users\Tmiracle\.codex\worktrees\xml-language-tools\Editor`，分支 `codex/xml-language-tools`。前阶段已交付基线为 `2385535bbf8b5c4f6c4074440357c0d8867ddf52`。
- 02 与 03 各自完成行为验收，SDK、宿主集成和 XML `0.2.0` 的共享变更集中检查一次，再一起提交。03 不依赖 02；共享交付不新增产品阻塞关系。
- 不重复 01 已通过且本阶段未改变的完整 Schema 网络、安装及 Image 独立生命周期矩阵。最后只运行一项 SVG/XML 短组合场景，观察未保存格式化、配对编辑、大纲定位、独立预览、Undo/Redo 和工作区布局恢复。
- 所有原生 Cargo 命令使用本工作树 `target/native` 和明确的 MSVC/Windows SDK 库路径；编译、链接及 GPUI 执行串行。源码修改全部留在隔离工作树，原工作区其他任务的改动未纳入交付。

## 共同检查

最终源码冻结后执行：`cargo fmt --check`、独立 guest 源码格式检查、`cargo test --workspace --exclude editor-app`、`cargo check --workspace`、全体非 ignored `editor-app` 测试、短组合 ignored 测试、当前宿主构建与公开 SDK 分发验证。只在新变更、失败或未解决问题影响相应路径时追加针对性重跑。

首轮实际记录：

| 检查 | 结果与边界 |
| --- | --- |
| 格式 | workspace 与 79 个本次修改/新增 Rust 源码检查通过；独立 guest 源码另纳入检查 |
| 非 UI workspace | 202 passed、0 failed、173 ignored，91.60 s；ignored 不计为通过 |
| workspace 编译 | 通过，73.24 s；保留已有 unused/dead code/MSVC/tree-sitter 警告 |
| 全体非 ignored App | 370 passed、1 failed、160 ignored，133.70 s。唯一失败为 `plugin_dock_layout_survives_delayed_startup`，旧 fixture 的完整预期缺少 Explorer 旁的大纲；实际所有原有面板、外层尺寸、插件信息与关闭状态均保留。更新预期只允许原 Explorer 内新增等分 Outline，并保留完整树和既有尺寸的严格比较；尚待重跑 |
| 首次链接环境失败 | 非 UI 首次链接报 `LNK1104: msvcrt.lib`，测试尚未运行；固定路径的库文件存在，独立最小 rustc 链接预演成功。使用明确 LIB 和单构建任务重跑通过；未确定最初链接失败的根因，不归为产品测试失败 |

以上结果对应语义关联扩展加入前的源码，作为首轮历史证据保留。后续 `language.editing 1.1` 的受影响检查与共同门禁如下，不把旧结果冒充新契约的验收。

后续实际记录（包含语义关联扩展和布局 fixture 修正，先于第二轮格式化器配置修复）：

| 检查 | 结果与边界 |
| --- | --- |
| 格式 | workspace 与本次修改/新增 Rust 源码检查通过；独立 guest 源码另纳入检查 |
| 非 UI workspace | 202 passed、0 failed、173 ignored，295.27 s；本轮重新编译受公开契约影响的测试产物。ignored 不计为通过 |
| workspace 编译 | 通过，8.63 s；未改动无关的既有警告 |
| 全体非 ignored App | 374 passed、0 failed、161 ignored，测试 127.45 s、命令合计 138.76 s；旧布局迁移完整树比较已通过 |
| 当前宿主构建 | 通过，120.95 s |
| 公开 SDK 分发 | `./scripts/verify-plugin-sdk.ps1 -HostExe target/native/debug/editor-app.exe`：通过，66.66 s；导出、修复缓存、头文件和仓库外独立 guest 构建均通过 |
| 三个正式语言包 | XML `0.2.0`、HTML/JavaScript `0.3.0` 构建通过，24.54 s；源清单、ZIP、原生资源、当前 XML 组件及固定候选 Git blob 哈希一致 |
| 短 SVG 组合首轮 | 0 passed、1 failed，100.96 s，等待 helper 未报告具体阶段。已增加阶段与原生文本、树/预览边界、提供者和场景诊断，尚待复现；未据总时长推断冷启动或产品根因 |

SDK/协议和非 UI 模块在后续格式化器配置修复中未改变时复用对应成功门禁；App 新改动另行执行受影响原生回归与最终常规测试。短组合失败日志保留为 `final-svg-combination-before-phase-diagnostic.log`。

短组合诊断与配置修复增量：

- 带阶段诊断的同一精确函数：0 passed、1 failed，98.55 s，`final-svg-combination-key-context-red.log`。初始结构与预览均可见、XML formatter 正常选中，失败明确为 `manual formatting`。GPUI 的 `!PluginSurface` 会检查全部祖先上下文，而原生编辑区被嵌入该插件布局，因此文档键位被排除。宿主源码 wrapper 新增 `NativeEditorSource`，文档键位通过 `EditorShell > NativeEditorSource` 接纳该宿主后代，不扩大外层 guest 控件权限。组合同时观察 guest/source 的公开 `bindings_for_action_in`，不以直接派发 Format action 绕过缺陷。
- 组合还等待插件公开树标签变为新名称，防止旧 revision 的相同路径提前满足；工作区恢复等待异步树结果，不把单帧 draw 当作完成。第一次辅助标签函数漏 `SharedString` import，编译报 E0433，未执行测试；已补 import，日志 `final-svg-combination-source-context-build-red.log` 保留。
- 无效格式化器配置：完整原生回归 1 passed、0 failed，8.07 s，旧存档/User 入口回归 1 passed、0 failed，0.01 s；未知显式选择停止 manual/save 格式化、普通保存保留文本、原生设置可重置、有效候选撤销仍遵守接替规则。详见 02 与 `02-persisted-formatter-final.log`、`02-persisted-archive-green.log`。
- 最小原生上下文修复后同一短组合完整 GREEN：1 passed、0 failed，98.93 s，含编译命令合计 171.95 s，`final-svg-combination.log`。正式 XML 安装读回经批准本机工具值/来源，23.40 s；未保存格式化、直接两端修改、大纲新名称及点击精确位置/编辑焦点、预览原始 Unicode 文本和真实 raster、一次 Undo/Redo、Dock 完整树与恢复后的实际 Explorer/Outline 同时显示全部通过。guest 焦点无 Format/Rename/Save 文档键位，嵌入的宿主源码焦点三个键位均存在；真实 ShiftAltF 达到格式化结果。源码与公开包身份为下述最终固定候选。

最终 App 修复后的格式（2.03 s）、本次独立 guest/新增 Rust 格式（0.48 s）和 workspace check（5.03 s）均通过；常规 App 全套 375 passed、0 failed、162 ignored，测试 130.72 s、命令合计 142.39 s；当前生产宿主构建通过，42.33 s；`git diff --check` 通过。日志在删除重复校验前另保留为 `final-app-suite-before-duplicate-validation-cleanup.log`。`6a243ee...167168d` 对公开协议、plugin-runtime、SDK_FILES 与四份读者协议正文的差异为空，因此该轮复用相同内容的非 UI/SDK/网站检查。

随后规范轴发现并移除一条重复纯清单校验；这次实际修改 runtime 源码，因此重新运行非 UI workspace、App 常规测试、check 与生产宿主构建，不能再称非 UI 源码未改变。公开 SDK/插件源码及内容仍未改变，长 Schema/格式化/标签/Dock 矩阵不重复。

删除重复校验后的最终门禁：

| 检查 | 最终实际结果 |
| --- | --- |
| `cargo fmt --check` | 通过，2.06 s |
| 本次 Rust 与独立 guest `rustfmt --check` | 通过，0.47 s |
| `cargo test --workspace --exclude editor-app` | 202 passed、0 failed、173 ignored，212.20 s；ignored 未计为通过 |
| `cargo check --workspace` | 通过，4.14 s |
| `cargo test -p editor-app --no-default-features -- --test-threads=1` | 375 passed、0 failed、162 ignored，测试 149.01 s、合计 189.79 s；真实包 ignored 结果分别见详细记录，不宣称全部 ignored 执行 |
| `cargo build -p editor-app` | 通过，10.47 s；最终生产宿主 SHA-256 `F85016AE6FD7F90C35373B1756354E2BB3A8AF6A54B3FBFE22240E98245CE489` |
| `git diff --check` | 通过，0.11 s；含未跟踪文件的候选 index 另通过 `--cached --check` |
| 当前生产 SDK 内容 | 当前宿主 `--export-plugin-sdk` 导出成功；结构/语言 Rust 模块原字节及两份按离线规则转换的 SDK 文档与源码一致，`final-sdk-content.log`。首次直接比较网页源文件的辅助断言忽略去元数据及链接转换，故不成立；按既有公开转换规则核对通过，无产品源码修改 |
| 当前生产原生窗口 | `./scripts/svg-startup-smoke.ps1 -HostExe target/native/debug/editor-app.exe -CompanionPackages @('dist/plugins/xml.zip')` 通过，合计 99.69 s；实际 SVG/XML 正式包、生产宿主启动、正常 WM_CLOSE 退出及插件状态保存均通过。`capture=False`，不宣称获得截图；原生输入、布局与绘制的证据来自上述 GPUI 场景 |

删除重复纯校验不改变包准入结果、SDK、协议或任何插件/原生输入源码；已通过的 SVG 组合、两端输入、Schema、格式化、树及四向停靠详测按相同内容复用。当前正式产物再次与最终固定候选逐字节回查：

| 产物 | 版本 | ZIP SHA-256 |
| --- | --- | --- |
| XML | `0.2.0` | `5B31A035C5508BDD2E91D97BCA667FF7034A637B394EBD1E7210AC2946B8D615` |
| HTML | `0.3.0` | `9100A53935C8DF5784FCAD7233CAF6F8B382FB13C0A06037E83DDB5913F9EC10` |
| JavaScript | `0.3.0` | `64957D9F902DF7445DC7BE8A8FD17CBDE79EE74FEA6C6B27FC16783B9E94CE98` |
| 独立 SVG 组合包（未修改） | `0.5.0` | `D39BB0F3D3AF7A043ECBCA53E954B0EF59C78CA67B7CC2EF3638F33A17300F36` |
| 陌生 SDK 验收组件 | `0.17.0` | `66FE6B15B6A1D7DA653A66134315E3D5D82AC71D1DA2B51E5393158FC74A3811` |

XML `xml.wasm` 为 `E937BD84073F642A28CBD0DEE5EA4CCC0D82FD776137AC07D50E7E8CAFB090F1`；HTML/JS native bundle 分别为 `F4BDB38E96A0F996BF49EA94B2E11C11EE4922149192EBE5FA031615795F9EF1`、`AC1532E4F0A071FE709C32A3C57F65471C24D6C7D6BC62BAF44DDFEB8FC865C6`，均与清单、正式 ZIP 和 Git 原始 blob 一致，JS 独立 NOTICE 原字节也一致。ZIP 仅含运行资源，不泄漏 SDK 副本、`src/`、`service/`、`node_modules/`、Cargo/npm 构建清单或 `target/`。

日志在 `target/xml-language-tools/final-*.log`；生产窗口夹具为 `target/svg-smoke-466eecca0b4742dca786810e82c2a8ed`。首次 App 失败另保留为 `final-app-suite-before-outline-migration-fixture.log`，首次链接失败保留为 `final-non-ui-first-link-failure.log`。

网站协议正文为中英文内容变更。已执行 `npm test`：15 passed、0 failed、2 skipped；两个搜索测试因没有站点构建索引跳过，不计作通过。最终文件链接、包版本/资源/hash 与 `git diff --check` 另行核对。

## 审查与交付

共同检查完成后，以不可变候选提交相对上述基线分别运行 Standards 与 Spec 独立审查；保留两轴原始结论、必要修复和最终候选身份。普通提交推送后核对远端 SHA，再关闭准确的 #74、#75 并读回状态；父设计议题 #72 不修改、不关闭。

第三轮复审固定候选 `167168dac4347cf7ba7e75fa3cebc73be723b73b`，146 个文件、tree `05645a3565e5ef5a9557f4c801859c30e12fa8e1`，父提交为同一已交付基线。记录比较命令 `git diff 2385535bbf8b5c4f6c4074440357c0d8867ddf52...167168dac4347cf7ba7e75fa3cebc73be723b73b` 与 `git log 2385535bbf8b5c4f6c4074440357c0d8867ddf52..167168dac4347cf7ba7e75fa3cebc73be723b73b --oneline`；引用可解析、非空差异及提交列表均核对。

最终复审固定候选 `8e92b61b158fd61f2c1f014f5e8459585897400e`，146 个文件、tree `d65dddedea7cd464f0fdaf0064a33a22e188430d`，同一父提交；相对第三轮唯一源码差异是删除上述重复校验，其他变化仅为验收记录。`git diff 2385535bbf8b5c4f6c4074440357c0d8867ddf52...8e92b61b158fd61f2c1f014f5e8459585897400e` 与对应 `git log` 的引用、非空差异和提交列表均核对；两个独立轴基于原审查只复查该增量，期间不修改审查源码。

首轮审查固定候选 `6b1fd883227bda986805e5c6f0d89a79cd002f62`，父提交为 `2385535bbf8b5c4f6c4074440357c0d8867ddf52`。比较前核对引用可解析、差异非空与提交列表；该候选由独立临时 index 生成，未把未验收源码提前提交到分支。

### Standards

首轮 1 项 P2 硬性资源问题：JavaScript bundle 内含 TypeScript 的 Unicode 数据，但打包仅保留主 LICENSE，漏掉其 `ThirdPartyNoticeText.txt`。收集脚本已补充 NOTICE 与第三方声明文件，锁定依赖的原文件复制到正式资源目录；JavaScript bundle 字节与哈希未改变。最终 ZIP、源文件、锁定依赖和候选 Git blob 的原始声明字节 SHA-256 均为 `1AF3C68039C57E539422DA82A4FAADA506CE6D0EA6F90E0B699D02DBCDB7A90C`。判断性坏味道 0 项。

第二轮固定候选 `6a243ee5ff49ff9a43c3f88e1d10966cd2ad6ffb`（同一父提交）的 Standards 独立结论：硬性发现 0 项、判断性坏味道 0 项。仍需核对下一轮 App 配置修复的最终候选。

候选 `167168dac4347cf7ba7e75fa3cebc73be723b73b` 的独立原文结论：硬性规范违规 0 项；判断性坏味道 1 项、最高 P3。`crates/plugin-runtime/src/package.rs` 的新增 hunk 连续两次调用 `crate::structure::validate_manifest(&manifest)?`；纯清单校验重复相同检查，没有新增边界保障，属于 Duplicated Code，非硬性违规。已删除第二次调用，保留原有首次结构校验；新固定候选的复审与受影响门禁进行中。

最终候选 `8e92b61b158fd61f2c1f014f5e8459585897400e` 的独立原文结论：硬性违规 0 项、判断性坏味道 0 项，最高优先级无。确认仅保留首次结构清单校验，原 P3 已解决；其他源码未变化，验收记录区分已通过检查与删重复后的 pending 门禁。本轮只读复查增量，未运行 Cargo、原生应用或打包。

### Spec

首轮 1 项 P2：合法 HTML `<DIV></div>` 的官方解析器确认配对，宿主原先只允许两个相同初始文本范围，因此直接编辑只修改一端。标准 LSP 本身要求关联范围具有相同初始文本，不能直接放松标准响应。新增公开、版本化、双方协商的语义关联方法，由插件确认配对，宿主按其选定方法校验并同步名称；标准方法仍保持相同文本要求。陌生插件 ID 的实际 HTML 输入、Undo/Redo、IME commit/cancel 回归通过，标准非法响应和未经协商的伪标记仍拒绝，详细日志见 02。

第二轮固定候选 `6a243ee5ff49ff9a43c3f88e1d10966cd2ad6ffb` 的 Spec 独立结论：首轮 HTML 问题已解决，新增 1 项 P2。用户/项目设置手工写入未知格式化器 ID 后，被过滤为无选择并静默回退；原生 ShiftAltF 和 CtrlS 实际触发两次替代请求，`02-persisted-formatter-red.log`：0 passed、1 failed，4.96 s。修复区分曾经有效选择的正常撤销与未知显式配置，提供对应作用域的错误和重置；完整原生及旧存档/User 回归均 GREEN，最终候选审查尚待完成。范围蔓延 0 项。

候选 `167168dac4347cf7ba7e75fa3cebc73be723b73b` 的 Spec 独立原文结论：缺失/部分实现 0、范围蔓延 0、错误实现 0，最高严重程度无。有限 formatter 验证记录区分显式错误与正常撤回，manual/save 拒绝回退、作用域/ID 可见且可 Reset；已核对实际请求计数、磁盘原文、重载、单候选接替与多候选选择的 GREEN 记录。嵌入宿主源码使用后代上下文恢复文档键位，guest/source 的真实焦点断言符合 SVG 组合与键盘要求，未发现授权扩大；HTML 双边语义协商和标准同文本约束仍保持。该轴仅只读检查固定候选，没有自行运行 Cargo/原生程序或把 pending 门禁当作缺实现。

最终候选 `8e92b61b158fd61f2c1f014f5e8459585897400e` 的独立原文结论：Spec 缺失/部分实现 0、范围蔓延 0、错误实现 0，最高严重程度无。首次结构校验完整保留提供者数量、标识、WASM、权限与能力声明检查，符合规格第 157 行，无 Spec 回退；此前两项 P2 的已修复结论保持。本轮仅只读核对候选增量，未运行 Cargo 或原生程序。

最终两轴汇总：Standards 硬性 0、判断性 0，最高优先级无；Spec 0，最高严重程度无。

Windows x86_64 为实际验收平台。macOS/Linux 的原生服务分发、UI 和停靠未验收；此次隐藏生产窗口启动检查未获得有效截图，只证明启动及退出/状态保存，不能冒充视觉截图验收。
