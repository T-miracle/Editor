# 可替换格式化与标签编辑工单 02 验收记录

日期：2026-10-07。对应 [#74](https://github.com/T-miracle/Editor/issues/74)、[工单](../../tickets/xml-language-tools/02-format-and-tags.md)与[方案](../../specs/xml-language-tools.md)。

状态：本单针对性行为验收、最终共同门禁、SDK 分发、SVG 短组合与两个独立审查轴均通过；Spec 语义配对及未知格式化 ID 两项 P2 已解决。验收完成，待普通提交、推送及工单关闭读回；最终命令、产物与审查身份见[共同交付记录](02-03-delivery.md)。

## 验证边界

- Windows x86_64 MSVC；独立工作树 `codex/xml-language-tools`。所有原生 Cargo 设置 `CARGO_TARGET_DIR=target/native`，与工单 03 串行安排链接和 GPUI 执行。
- XML `0.2.0` 使用 01 已验收的受管原生 LemMinX `0.31.2`，无需 JRE；本单不重复完整 Schema 网络和 01 安装矩阵。
- HTML、JavaScript 包提升至 `0.3.0`。受管 Node `24.19.0` 与包内独立 stdio 脚本经固定 SHA-256、安装权限及私有版本目录准备；不依赖用户全局或项目 Node。HTML 使用 `vscode-html-languageservice 5.5.0`，JavaScript 使用 `TypeScript 5.8.3`。
- `language.formatting ^1` 是独立提供者角色，`language.editing ^1` 复用标准 LSP prepareRename、rename、linkedEditingRange；1.1 增加面向全部插件的协商式语义关联方法。HTML 要求 `>=1.1, <2`；XML 沿用标准路径。主分析、高亮和格式化选择独立；相同服务计划共用受控进程，不建立语言/插件 ID 白名单。
- 原生 `EditorState` 保持文本、IME、选择与 Undo/Redo 的唯一真相；关联范围与有界输入命令仅保留不可变快照/元数据。所有完整编辑方案先检查目标 incarnation/revision、UTF-16 边界、重叠、配额和提供者生命周期，再应用一次原生事务。
- 待配对输入最多等待 500 ms，语义等待预算为 128 条命令/64 KiB。超过任一上限立即取消配对，将同一个 FIFO 交还 Base；大平台粘贴不截断，原生历史微任务后的尾部在本次 event 结束排空，预算不代表原生载荷全程硬 64 KiB。Undo/Redo 按进入顺序交给 Base 原生历史；设置关闭、服务撤销/替换及选区改变只撤销配对，已输入字符必须落回原来的原生实体。Save、Format、F2 等待原生 Change 处理后读取文档版本。
- 跨功能 GPUI 用例位于 `crates/editor-app/src/tests/`，从实际 ZIP 经公开 Manager、现有生产发布夹具进入宿主。陌生格式化与慢语言包拥有独立 ID、声明、受管脚本和实际 JSON-RPC 日志，没有新增仅供测试调用的生产 API。

## 已执行记录

| 验证 | 实际入口与结果 |
| --- | --- |
| 独立能力及公开包契约 RED/GREEN | `cargo test -p plugin-runtime --test language_editing`：1 passed，0 failed，8.70 s；资源包 `primary=false/formatting=true` 经 Package→Manager 协商格式化能力；未声明能力的包被拒绝 |
| HTML/JavaScript 服务源码打包 | `./scripts/build-language-services.ps1`：GREEN，独立锁文件依赖与 esbuild bundle 生成完成；实际包保留 runtime 依赖许可证和锁定 hash |
| HTML/JavaScript 正式 ZIP | `./scripts/build-plugins.ps1 -HostExe target/native/debug/editor-app.exe -Packages html,javascript`：GREEN，`dist/plugins/html.zip` 与 `dist/plugins/javascript.zip`；使用宿主导出的公开 SDK/声明校验，不引用 workspace 内部 SDK 路径 |
| 当前 XML 格式配置 ZIP | 主代理执行 `./scripts/build-plugins.ps1 -HostExe target/native/debug/editor-app.exe -Packages xml`：GREEN，4.05 s；`02-current-xml-build.log`，包含两项公开格式枚举和配置转换 |
| 原生程序集编译 | `cargo test -p editor-app --bin editor-app --no-default-features --no-run`：GREEN；实际服务和原生交互不会以编译通过替代验收 |
| Windows 受管 argv RED | 慢语言包公开安装后，实际 Node 进程 `Module._findPath` 对 `\\?\C:\...\server.cjs` 抛出 EISDIR；正式 HTML/JavaScript 使用相同声明，也受影响 |
| Windows 受管 argv 修复 | 只对已规范化并验证归属的 `${dependency:id}/path` 展开值，转换子进程路径拼写并再次确认同一目标；不解析普通参数。重跑实际 Node 后 initialize 成功；该轮完整交互仍 RED，不能报告为全部通过 |
| 慢语言夹具定位 | 使用实际 Manager 的测试根改为现有 `.runtime-plugin-test`，避免文件识别加载另一个目录。增加文件识别与 Bridge 存在断言；协议录制证明此前只有 initialize、没有文档同步/配对请求 |
| 原生测试时钟定位 | GPUI TestDispatcher 的 timer 使用虚拟时钟；测试显式 advance_clock 后再绘制，与外部实际服务等待并行。新 caret 后不等待或预先请求配对，避免预热首字输入回归 |
| 原生捕获动作修复 | GPUI capture_action 默认传播；挂起输入时的 Undo/Redo、粘贴和删除必须显式消费已入队命令，随后只通过 Base 原生动作重放 |
| 慢输入实际中间结果 | 首字配对→Undo/Redo、排队粘贴/删除与完整配对历史、关闭端首次 IME、含 Undo/Redo 的切换标签、真实 canonical close 的 pending flush/dirty 保留、替换提供者后的 XY 顺序已观察到正确结果；该轮后续 Save/格式化仍 RED，不列为完整通过 |
| 原生回退历史预期修正 | 连续普通 XY 沿用 Base 合并为一次 Undo，不能要求人为拆成只退 Y；测试改为一次 Undo 完整还原、Redo 保持顺序 |
| Windows 原生关闭夹具修正 | open_file 将 tab identity 规范化为 verbatim 路径；关闭直接捕获实际 active_path，不使用 tempdir 的普通路径比较。重开检查真正换成新的 EditorState entity，避免未关闭的旧文档冒充 incarnation 验收 |
| 命令键位夹具 RED | main.run 才注册 EditorShell 键位，早期测试只初始化 Base，Ctrl+S/ShiftAltF/F2 未发出请求；早期 Escape 不打开输入框不算取消验收。正式键位表抽为生产初始化共用函数，测试复用并要求观察实际 prepare 请求后再 Escape |
| 慢语义服务完整原生 GREEN | `cargo test -p editor-app --bin editor-app --no-default-features tests::language_editing:: -- --ignored --nocapture --test-threads=1`：慢语言用例 passed，联合两项合计 1 passed/1 failed，25.34 s；`02-native-keys-green.log`。观察完整首字→Undo/Redo、排队粘贴/删除配对历史、首个关闭端 IME、实际 prepare→Escape、防迟到输入框、等待 Save/Format/F2 后切换、含历史的 tab 切换、canonical close dirty、快速设置关闭/切换和禁用提供者输入顺序、全局/语言覆盖、真实 Ctrl+S |
| JS 实际行为中间结果 | 同轮已完成标准 TS 格式化、不同 ID 替代/一次调用、主补全和 grammar 保持、用户/项目优先、新安装不抢占、Save 默认关闭及开启。随后 staleTyping 用例 RED：发出请求后测试做两帧重绘，150 ms 方案已先合法应用，再输入。改为请求之前准备 caret，请求之后立即输入；剩余生命周期/失败场景仍待执行 |
| 旧语言独立格式化文档 RED | `cargo test -p editor-app --bin editor-app installed_javascript_formatter_can_be_replaced_without_changing_analysis -- --ignored --nocapture --test-threads=1`：RED，10.31 s，`02-role-retirement-red.log`；实际 JS 主分析与另一个 ID 的格式化器已经各自打开文档，改 `.js` 关联后旧 main lease 撤销而独立 formatter lease 仍活跃 |
| 原生迟到操作夹具修正 | GPUI `simulate_keystrokes` 会 drain scheduler，不能用于“请求之后立即改变目标”的两个事件间隙。改为同一 Window 更新连续 dispatch 原生按键，再 drain；Undo 后仍 dirty，先普通 Ctrl+S 再 canonical close，确认真正分配另一 native entity |
| JS 整项 GREEN | `cargo test -p editor-app --bin editor-app tests::language_editing:: -- --ignored --nocapture --test-threads=1`：JS 函数 passed，联合 1 passed/1 failed，41.00 s；`02-native-ownership-red.log`。整项覆盖标准 TS、陌生 ID 替代及只一调用、主补全/grammar 不变、有效选择与项目优先、Save 默认关闭/启用、真实 stale typing、关闭重开、禁用、移除回退、显式无效工具不回退和非法重叠整提案不部分提交；重新关联同时撤销两个独立 lease，实际两个进程各收到恰一次 didClose，其他 JS 的服务仍保留 |
| XML 两项实际 RED | 已链接 EXE `installed_xml_ --ignored --nocapture --test-threads=1` 意外同时选择 01 的一个 native completion 用例：该项 passed，02 两项 failed，总计 1 passed/2 failed，359.56 s；`02-xml-native.log`。未重复 01 完整 Schema/安装矩阵，后续使用完整函数名过滤 |
| XML Unicode 应用坐标定位 | 首个 qualified 标签输入前未请求/等待配对。属性中的 emoji 使 Base 0.7 的 scalar 列与标准 UTF-16 不同，实际两端偏移，出现额外字符/缺失 `>`。仅在 format/rename/linked 的最终 Base 应用处做严格 UTF-16→byte→scalar 转换；公开协议和既有 text-drag 原生坐标保持原语义 |
| XML 属性默认定位 | 默认缩进、混合文本与 xml:space 原字节保留、手动未保存及一次 Undo/Redo、空元素 expand 已在真实 LemMinX 中走过；属性 splitNewLine 实际零缩进。按固定 0.31.2 源码补齐配置构造器未初始化的 formatter 默认，明确属性缩进 2 级，并保留 grammarAwareFormatting/preserveSpace |
| 关联切换待输入 RED | 同一慢语义场景增加同一行 emoji 与 X pending→`.linked` 关联 novel→Y→Ctrl+S；实际仍原文、两字符丢失。`02-native-ownership-red.log` 的失败属于新覆盖；Bridge 已改为通过现有 OpenTab.owns_editor 确认 native 目标，语义 lease 撤销只取消配对，完整 GREEN 待执行 |
| 慢语义服务与 native ownership 整项 GREEN | `cargo test -p editor-app --bin editor-app delayed_linked_service_preserves_first_input_history_save_and_retirement -- --ignored --nocapture --test-threads=1`：1 passed、0 failed、16.94 s；`02-native-ownership-green.log`。包含此前完整慢输入/历史/IME/保存/退役行为，以及待输入 X→关联撤销→Y→Ctrl+S 的顺序、实际磁盘、一次原生 Undo/Redo；同一行 emoji 也覆盖最终 native 坐标转换 |
| 固定 XML formatter defaults 后的实际重跑 | 精确 `installed_xml_formatting_preserves_text_and_honors_project_options --ignored --nocapture --test-threads=1`：仍 RED，185.04 s；`02-xml-formatting-green.log`。属性实际为 8 空格（2 级 × tabSize 4），测试误当作 2 空格；其余此前经过的默认混合文本/xml:space/原生历史和 expand 仍正确，已修正预期；collapse 与 XML Save-on 尚未走到，不宣称通过 |
| XML/HTML 标签完整函数增量 | 精确 `installed_xml_html_tags_share_native_rename_linked_input_and_undo --ignored --nocapture --test-threads=1`：RED，87.00 s；`02-xml-html-tags-green.log`。实际 XML 首字 qualified 名称、同一行 emoji、Undo/Redo、语义配对、自闭合、普通相似文本、关闭端粘贴及两端删除全部走过；F2 字段已可见，确认后未达到预期。后续 HTML/IME 尚未走到，不宣称通过 |
| F2 确认与标准提案 RED | 陌生语言包的实际字段输入、Ctrl+A/复制后 Enter：`02-rename-guard-red.log` 记录确认事件时名称为 0 字节，`02-versioned-response-red.log` 随后记录真实 rename 一次但 versioned 提案被拒绝。Base 的单行 Enter 传播后平台发送换行，经单行过滤为空并替换所选名称；局部名称栏父级立即消费 Enter，IME 标记状态仍归 Base/platform。正式 LemMinX 使用标准 versioned TextDocumentEdit，不能将其当资源操作拒绝 |
| 标准 WorkspaceEdit 整案校验 GREEN | 已链接 EXE `language::navigation::editing::tests:: --nocapture --test-threads=1`：3 passed、0 failed，0.00 s；`02-rename-proposal-contract.log`。`changes`、同文档版本化和 null version 均通过；不符 wire version、跨文件、资源操作、mixed、注释及重叠方案整体拒绝，不部分写入 |
| 名称栏真实原生交互 GREEN | 已链接 EXE `tests::language_editing::linked::public_rename_field_owns_typing_confirmation_and_native_history --ignored --nocapture --test-threads=1`：1 passed、0 failed，4.05 s；`02-rename-focus-green.log`。陌生公开包：typing、Backspace/Delete、Ctrl+X/V、字段 Undo/Redo 全部只变名称栏，源码保持；全选状态 Enter 发出恰一次 versioned rename，配对源码一次原生 Undo/Redo。捕获动作先核对 source focus，避免祖先监听吞掉字段输入 |
| XML formatter 完整 GREEN | 已链接 EXE `tests::xml_formatting::installed_xml_formatting_preserves_text_and_honors_project_options --ignored --nocapture --test-threads=1`：1 passed、0 failed，246.38 s；`02-xml-formatting-final.log`。默认 Save-off、手动未保存、mixed text/xml:space 原字节、原生一次 Undo/Redo、属性 8 空格（2 级 × tabSize 4）、empty expand/collapse、XML Save-on 的原生粘贴→Ctrl+S→实际磁盘均通过。公开 Project 本机工具设置在正式替换后读回同值、同来源，公开安装耗时 21.16 s；耗时本身不作为实际进程路径证明 |
| XML/HTML 标签本轮启动 RED | 同一精确函数 `--ignored --nocapture --test-threads=1`：0 passed、1 failed，75.32 s；`02-xml-html-tags-final.log`。prepare_until_ready 返回 paused，当轮未打印语言标识，不能推断为 XML；增加原有 recovery/runtime stderr 日志观察，不将中间通过冒充整项通过 |
| 正式 HTML 漏打包模块 RED | 新诊断同一函数 0 passed、1 failed，80.58 s；`02-xml-html-tags-recovery.log`。全部 XML QName/普通 F2、IME、两端输入、历史与设置场景已走完，在 HTML prepare 失败。实际受管 Node 24.19.0 stderr `Cannot find module './parser/htmlScanner'`：官方固定包 default main 使用 UMD factory，形参 require 使 esbuild 漏掉相对模块。以官方 package.json 声明的 ESM module 入口静态打包，版本不变 |
| HTML ESM 正式包与 stdio GREEN | `./scripts/build-language-services.ps1 -Packages html`、`./scripts/build-plugins.ps1 -HostExe target/native/debug/editor-app.exe -Packages html`：GREEN。单独本机 Node 24.19.0 预演正式 bundle 的一个 stdio 会话 initialize/linked/prepareRename/rename/format/shutdown：全部 GREEN，`02-html-esm-stdio.log`；这个开发预演不替代公开 Manager 的私有 Node/原生 UI 验收。新 bundle SHA-256 `db7fc77cd605f93df6230378b71b7d8bbc20e78ce86ae3b760e6c0af2ddf8cd9` 与 manifest 相同 |
| FIFO 上限实际 RED/GREEN | `cargo test -p editor-app --bin editor-app --no-default-features bounded_linked_queue_keeps_overflowing_input_after_native_history -- --ignored --nocapture --test-threads=1`：原实现 RED 19.33 s（编译 35.05 s），Y/Z 丢失后实际仍原文，`02-queue-budget-red.log`；同 FIFO 修复后 1 passed、0 failed，48.28 s（编译 39.67 s），`02-queue-budget-green.log`。冷首字→Undo→128项附近→同 update Y/Z、第129项为 Redo、64KiB实际 clipboard paste→Y，逐段检查文本与完整 Undo/Redo。取消语义后 FIFO 仍可见，defer 只携带 Base history 动作而不持有/覆盖另一个尾部队列 |
| FIFO 修改后原生交互完整复核 | 已链接 EXE `tests::language_editing::linked:: --ignored --nocapture --test-threads=1`：2 passed、0 failed，22.24 s；`02-native-fifo-focus-final.log`。原完整慢服务场景和版本化 F2 名称栏全部通过，不重复已 GREEN 的 JS 或 XML 格式化长场景 |
| 原始 changes 注释拒绝 GREEN | 已链接 EXE `language::navigation::editing::tests:: --nocapture --test-threads=1`：3 passed、0 failed，0.00 s，`02-rename-proposal-contract-final.log`。增加原始 changes[].annotationId 非法值，不能经 serde 丢弃未知字段绕过未协商注释拒绝 |
| XML/HTML 正式整项 GREEN | 已链接 EXE `tests::tag_editing::installed_xml_html_tags_share_native_rename_linked_input_and_undo --ignored --nocapture --test-threads=1`：1 passed、0 failed，77.03 s，`02-xml-html-tags-complete.log`。当前 XML 与新 ESM HTML ZIP 经公开 Manager/受管私有服务完成同一场景：XML declared QName 和无前缀、HTML普通 F2；两端首字/emoji/粘贴/删除；同名嵌套与 self-closing/void 不误改；普通属性/注释相似文本保持；首次关闭端 IME、预编辑/提交/取消及完整一次 Undo/Redo；按语言关闭联动、深浅主题与键盘。正式包运行不依赖全局 Node |

XML formatter 默认依据为固定版本的 [XMLFormattingOptions 源码](https://raw.githubusercontent.com/eclipse-lemminx/lemminx/0.31.2/org.eclipse.lemminx/src/main/java/org/eclipse/lemminx/settings/XMLFormattingOptions.java)。后续短回归复用 `target/xml-verification/native/lemminx-win32.exe`，其 SHA-256 为 `25f571c0f07d0ad76be60a86c1b864d46cf7bf94773dc21e5d5da646478aac79`，来自本轮及 01 已实际完成的正式分发。`tests/editing_fixture.rs::install_xml` 校验 hash，经资源声明 bootstrap 的公开安装和 `Manager::update_setting` 设置项目本机工具，然后重新安装未修改的正式 XML ZIP；不发布 bootstrap UI，不新增生产测试 API。不存在该已批准工具时仍按正常私有安装准备，hash 不符则拒绝复用。

版本化重命名依据为固定 LemMinX 0.31.2 的 [XMLRename](https://raw.githubusercontent.com/eclipse-lemminx/lemminx/0.31.2/org.eclipse.lemminx/src/main/java/org/eclipse/lemminx/services/XMLRename.java) 与 [TextEditUtils](https://raw.githubusercontent.com/eclipse-lemminx/lemminx/0.31.2/org.eclipse.lemminx/src/main/java/org/eclipse/lemminx/utils/TextEditUtils.java)，以及官方 LSP 3.17 的 [WorkspaceEdit](https://raw.githubusercontent.com/microsoft/language-server-protocol/gh-pages/_specifications/lsp/3.17/types/workspaceEdit.md) 和 [TextDocumentEdit](https://raw.githubusercontent.com/microsoft/language-server-protocol/gh-pages/_specifications/lsp/3.17/types/textDocumentEdit.md)。客户端只声明 documentChanges 文本能力；提案 URI/非 null wire version 与最终原生 incarnation/revision/source 两层校验，资源与注释能力不宣告。

HTML 新 ESM bundle 包含 `@vscode/l10n 0.0.18`；其 npm 包没有 LICENSE，随包保留对应 [l10n/v0.0.18 固定 commit 的 MIT notice](https://github.com/microsoft/vscode-l10n/blob/fc7e3d79ddb91a2cc24a9730aad912026a43dbf2/LICENSE)。源码打包脚本遍历 scoped package，已有手动固定 notice 在重复构建时保留。

日志位于 `target/xml-language-tools/02-*.log`。前期 `02-tags-red.log`、`02-tags-paint.log` 和慢服务 RED 日志均保留实际失败，没有把 ignored 或失败回归列为通过。

## 针对性验收映射

共同审查 P2 的增量按 [LSP 3.17 标准关联编辑](https://raw.githubusercontent.com/microsoft/language-server-protocol/gh-pages/_specifications/lsp/3.17/language/linkedEditingRange.md) 保留相同文本/长度要求，不能按 HTML 或大小写在宿主放松。公开 `language.editing 1.1` 通过严格 `experimental.meEditorSemanticLinkedEditing={version:1}` 双边握手启用 `meEditor/semanticLinkedEditingRange`；授权由运行时已协商能力和选中方法绑定，不信任响应 flag。语义范围允许不同初始名称/长度，但每端仍须满足非空、64 KiB、UTF-16、互不重叠、caret 和 wordPattern 校验。

| 语义配对增量 | 实际入口与结果 |
| --- | --- |
| 陌生 ID 正式 HTML 原生 RED | `cargo test -p editor-app --no-default-features unknown_html_semantic_pairs_preserve_mixed_case_cold_input_history_and_ime -- --ignored --test-threads=1 --nocapture`：0 passed、1 failed，14.23 s（编译 98 s）；`02-semantic-html-red.log`。公开 Manager 安装重标 `foreign-semantic-markup` 的正式 ZIP，冷首字仅将 `<DIV>` 改为 `<DXIV>`，`</div>` 不变，确认为配对被拒绝而非输入丢失 |
| HTML 增量正式打包及 stdio GREEN | `scripts/build-language-services.ps1 -Packages html` 与 `scripts/build-plugins.ps1 -HostExe target/native/debug/editor-app.exe -Packages html`：GREEN；HTML 为声明式资源包，此轮不构建 WASM 或新 Host。Node 24.19 单会话完成握手/标准 linked/语义 mixed-case/prepare/rename/format/shutdown，`02-semantic-html-stdio.log`。标准 mixed-case 返回 null，语义方法返回两个官方 parser 范围；新 bundle SHA-256 `f4bdb38e96a0f996bf49ea94b2e11c11ee4922149192ebe5fa031615795f9ef1` 与 manifest 一致 |
| 方法权限与范围契约 GREEN | `cargo test -p editor-app --no-default-features editor::linked_input::tests:: -- --test-threads=1 --nocapture`：2 passed、0 failed，0.00 s（编译 94 s），`02-semantic-contract-green.log`。标准不同文字范围及附加伪 semantic flag 拒绝；语义不同文本/长度通过，光标不在名称内、重叠、空范围、surrogate 中点、缺少模式、第二端非法模式/超额均拒绝整组 |
| 严格握手 GREEN | 已链接 EXE `language::navigation::startup::tests:: --test-threads=1 --nocapture`：1 passed、0 failed，0.00 s，`02-semantic-handshake-green.log`。未知版本、未知字段、无运行时授权及非标记形状均不启用语义方法；startup 同时移除插件 client_experimental 中保留 key，按实际协商的 editing 1.1 授权添加 |
| 陌生 ID 正式 HTML 原生完整 GREEN | 精确函数 `unknown_html_semantic_pairs_preserve_mixed_case_cold_input_history_and_ime -- --ignored --test-threads=1 --nocapture`：1 passed、0 failed，5.34 s（编译 59.96 s），`02-semantic-html-final.log`。同一正式 ZIP 重标未知 ID 后经公开 Manager/私有 Node：不同大小写的任意一端冷首字、逐次 Undo/Redo、首个 IME 提交及历史均通过；取消先实际观察当前端 `<D拼IV>`、另一端仍 `</div>`，再取消恢复初始两端，避免未处理队列导致的假通过 |
| 修改后 slow/F2 完整 GREEN | 已链接 EXE `tests::language_editing::linked:: --ignored --test-threads=1 --nocapture`：2 passed、0 failed，22.40 s，`02-semantic-slow-f2-green.log`。受影响延迟输入/IME/FIFO/退役/保存及原生名称字段版本化 F2 完整回归通过；使用未声明语义扩展的标准服务路径 |
| 双语 SDK 内容 GREEN | 英文先更新，再同步中文；`website/ npm test`：15 passed、0 failed、2 skipped（未构建站点搜索索引），`02-semantic-website.log`。新 capability 版本、固定 marker/method、授权与两种方法的范围限制逐条对应 |

此前 XML 格式化、JS 替代选择和 XML/HTML 完整正常标签矩阵的 GREEN 记录保留，不重复执行这些长场景。最终宿主 SDK 导出、正式包重建与公共门禁由主代理针对冻结后的全部变更集中执行。

| 范围 | 已通过的可观察证据 |
| --- | --- |
| T07–T08，格式化入口/XML策略 | XML 完整实际 formatter 用例：手动事务、默认 Save-off/启用 Save-on、磁盘排版、混合文本/xml:space 与两项公开配置 |
| T09–T10，独立与替代提供者 | JS 完整实际 Manager/原生用例：标准与陌生 ID 角色选择、项目优先、仅一调用、不抢占、移除回退、主分析/grammar 保持、显式无效错误 |
| T11，显式重命名 | 两个正式包完整 tags 用例；陌生包版本化 F2 字段交互用例；标准 changes/versioned/null/非法整案校验矩阵 |
| T12–T14，联动/编辑历史 | 两个正式包 tags 用例；慢服务完整 FIFO/首字/IME/设置/退役/保存场景；128项、129th历史与64KiB粘贴边界真实 RED→GREEN |
| T23–T26，本单增量 | 公开能力 Package→Manager、三个正式独立包、角色文档撤销与迟到结果、原生唯一状态/事务、SDK双语更新；01完整安装/Schema矩阵复用已通过证据 |

## 持久化显式格式化错误增量

第二项独立 Spec P2：UI `choose` 拒绝未知 ID，但原 `Saved` 加载与 `Registry::resolve` 会过滤合法 JSON 中手改的未知显式 ID，再按 automatic/sole 静默回退。新 `formatter_choices` 为每个用户或项目当前显式 formatter key 保留一个已验证值；只通过实际候选或 UI 校验记录，清除或手改后剪除相应旧记录，不维护历史插件集合，仍通过现有 1 MiB 配额与 NamedTempFile 原子保存。旧文件缺字段使用 default，匹配实际候选后补录。

已验证但后来不可用的显式值保持正常撤销处理，验证记录不被 automatic fallback 覆盖；从未验证或手改为其他未知 ID 时保留当前有效 User/Project 来源及 ID，设置行显示中英文配置错误。错误按 formatter key 保存，不设置 Registry 全局载入/写入 error，用户可通过该作用域的原生 Reset 清除。手动或保存格式化先拒绝当前错误，不能借用缓存服务或任何候选；保存仍写原生原文，同时保留磁盘冲突/失败提示与配置错误。识别、高亮及主语言服务的既有选择规则不改变。

| 持久化入口增量 | 实际入口与结果 |
| --- | --- |
| 公开 Manager/原生 RED | `cargo test -p editor-app --no-default-features persisted_formatter_typo_blocks_dispatch_and_native_reset_preserves_removal_rules -- --ignored --test-threads=1 --nocapture`：0 passed、1 failed，4.96 s，`02-persisted-formatter-red.log`。两个陌生 formatter 经公开安装，有效 User 与 Project 选择落盘确认；手改 JSON 的 Project ID 为 typo-never-installed/format，再公开 configure 重载并发布实际 Manager。Shift+Alt+F 与 Ctrl+S 向旧 Project formatter 实际发出 2 次 formatting 请求，期望为 0，证明静默回退 |
| 首次修复后测试窗口时序 RED | 同一精确函数 0 passed、1 failed，4.98 s（编译 71 s），`02-persisted-formatter-green.log`。前段零格式化请求、原文保存、错误及 Project 来源、真实设置错误/reset 恢复 User 已通过；Esc 已关闭 modal 后仍用 dialog VisualContext update，GPUI 报 window not found。只修测试时序：通过仍打开的主窗口确认 modal 已关闭 |
| 完整原生 GREEN | 同一精确函数 1 passed、0 failed，8.07 s（编译 65 s），`02-persisted-formatter-final.log`。未知 Project ID：manual/save 均零 formatter 请求、实际磁盘为原始用户文本、status 保留 ID/来源；原生 Languages 设置错误在明暗主题均可见，点击 Project scope + Reset 恢复 User，正常一次格式化和 Undo。公开 disable 后唯一候选接替、重载/重复刷新保持；新安装不抢占；重新选择后公开 uninstall，两个候选要求选择且无额外请求 |
| 旧存档与 User 错误 GREEN | 已链接 EXE `language::providers::formatting::tests:: --test-threads=1 --nocapture`：1 passed、0 failed，0.01 s，`02-persisted-archive-green.log`。旧文件缺新字段，实际候选完成迁移；sole fallback 后重载仍保持移除记录，automatic 不覆盖；手改 User ID 显示 User 配置错误且禁用选择，普通 choose reset 可成功，有限验证记录清空 |
| 范围与复用 | 已对本次 Rust 文件完成 rustfmt，正常仓库配置 `git diff --check` exit 0，`02-persisted-diff-check.log`。本增量未改公开协议、SDK 读者页/导出文件或插件包；既有 XMLfmt、JS 完整替代矩阵、长 tags、SDK/正式 ZIP 结果复用。主代理接收 native 槽后继续当前 EXE 的 SVG 组合诊断与最终全 App/check/fresh host |

## 共同交付检查待执行

- 主代理集中执行最终公共格式、非 UI workspace、workspace 编译、全 App 非 ignored 测试、当前 SDK 独立分发、短 SVG/XML 组合和双轴审查；结果到齐后更新交付/推送/议题状态。
