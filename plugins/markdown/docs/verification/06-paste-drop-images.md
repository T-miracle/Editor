# 06 — 图片粘贴与拖入验证记录

**状态：验收通过，Git 交付及 issue 关闭另读回记录。** 比较基线 `9f3670e`，Markdown 包 `0.6.0`，公共能力示例 `0.15.5`。

## 目标、前置与范围

2026-10-04 开始 GitHub [#32](https://github.com/T-miracle/Editor/issues/32)。已独立读回其 open / ready-for-agent 状态及直接前置 #30、#31 的 closed / completed 状态。05 已提交、推送并核对远端提交 `9f3670ed0185ac12a85a2c23a49148a8dbf80c2a`。沿 [06 工单](../tickets/06-paste-drop-images.md)覆盖 M09、M10、M03、M06、M16。

主接缝为真实独立构建 Markdown 包 → 公开 Manager → 原生源码区 Paste / 外部文件 Drop → 同级文件及可撤销引用 → 原生图片预览。新增能力同时通过独立能力夹具验证；宿主不按 Markdown ID、语言或扩展名分支。

## 实施决策与副作用

- 图片内容保留在有上限、实例与文档版本绑定的宿主资源中，避免 8 MiB 图片以 JSON 字节数组穿过 2 MiB 请求限制。
- 插件决定 `img`、`img1`、`img2` 等名称与相对引用；实际格式从字节确认。宿主提供受权限控制的原子 create-new，不覆盖文件。
- 保存前校验文档及目录边界；尚无已保存文件时先提示保存，不落盘。取消或写入失败不插入引用；只清理该请求创建但尚未完整写入的新文件。
- 完整文件保存成功后若文本版本过期、关闭或目标切换，分别报告文件已保存与引用未写入并保留文件。Undo 仅撤销文本引用，不删除图片。
- 不引入另一份可变文本或撤销栈。插入经 04 已交付的版本化原生事务及 DocumentSession Change 订阅。

## 命令与结果

- `editor.images 1.0` 只通知实际格式、字节长度及不透明句柄。剪贴板额外要求 `clipboard`，文件创建要求 `workspace.write`；要求有效文档版本、面板 opt-in、实例、工作区、信任与授权。单图 8 MiB、批次 8 张／32 MiB、管理器输入驻留 64 MiB、句柄期限 30 秒；原生准备／队列在复制或读取前保留配额。已受理写入持有不可复制的配额租约，撤销 UI 后仍可确认该次文件收据。
- `cargo test -p plugin-protocol images -- --nocapture` 最终 3 passed，记录 `target/06-protocol-final-green.log`。真实 SDK 夹具先因缺失协商能力 RED，再 GREEN；后续完整 9 项首轮是测试包漏声明 `editor.documents`，并非产品缺陷。第二轮 8 passed / 1 failed（`target/06-runtime-images-second-run.log`）；收据测试误把新探测请求替换了待观察任务，修正夹具后该项单独 1 passed（`target/06-runtime-image-receipt-green.log`）。未把失败批次报告为全部通过。
- Standards 审查发现撤回输入 opt-in 后、尚未 poll 的连续事件仍可提交旧句柄。新增第 10 项真实 WASM 回归实际 RED（`target/06-runtime-withdrawal-red.log`），将资源 reconciliation 移至新 UI 树发布之后；`cargo test -p plugin-runtime --test editor_images source -- --ignored --nocapture --test-threads=1` 3 passed、0 ignored（`target/06-runtime-withdrawal-green.log`）。与前述逐项验证合计 10 项行为通过：协商、权限、来源、格式／配额、过期、关闭、切换、撤回、实例替换、取消前后副作用及已受理保存收据。
- 平台附件创建 3 passed（`target/06-files-green.log`）：并发只产生一个完整文件、现有文件不覆盖、同级及安全路径限制、新文档保存 create-new。Windows 逐层持有不允许 DELETE 的目录句柄，以实际打开路径校验物理归属；临时文件写完并 sync 后原子发布，不完整临时文件仅清理自己的请求。保存前文档文件也保持句柄；不会经已替换 junction 写向外部目录。
- 访客命名、引用插入、CRLF 与上下文均先 RED → GREEN；嵌套在已有图片替代文字中的图片会被原生渲染隐藏，新增回归实际 RED（`target/06-guest-nested-alt-red.log`），修正只承认可见外层图片。完整独立访客 28 passed / 0 ignored（`target/06-guest-final.log`）。保留顺序、实际后缀、原选区外字节、一次 Undo；拒绝隐藏引用，不把文件收据当文本写入成功。长收据使用 56 px 原生滚动区域。
- 原生图片组 `cargo test -p editor-app markdown_tests::image_import -- --ignored --nocapture --test-threads=1` 最终首批 7 passed / 0 ignored，145.32 秒（`target/06-native-images-final.log`）。使用实际 `dist/plugins/markdown.zip`、系统 Paste action、GPUI 外部 FileDrop 与 Window 保存提示，在隔离目录观察文件、源文、选区、Undo、可见图片像素及双语收据。覆盖混合实际 PNG／JPEG、已有 img、连续批次、多图一次撤销、取消保存、首次保存、真实文件锁保存失败、写权限拒绝、IME、文件完成后的旧 revision 与关闭重开。
- 拖入最初因原生源码的遮挡命中规则无法触达父级 drop 回调，实际 RED 后改用上游公开 typed drag 事件与逐帧 Window 鼠标监听；未复制上游实现或建立宿主业务分支。只有当前外部拖拽、源码正文真实 caret 命中、有效版本与授权才消费拖入；工具栏／行号区不能退回旧选区。
- 第二次粘贴／拖入原先在准备期静默丢弃，并可能先改变第一批选区。实际 native RED（`target/06-native-busy-red.log`）后，统一入口明确提示重试，先检查忙状态再移动选区；最终 7 项组覆盖连续两个 Paste 加一次真实 FileDrop、第一批保留及可见提示。
- 增量 Standards 审查发现全局 capture 会穿过宿主浮层。实际文件树菜单阻挡回归 RED（`target/06-native-blocker-red.log`），统一捕获接缝检查菜单、重命名、删除、插件弹窗、dialog 与 sheet；新增第 8 项原生回归单独 1 passed / 0 ignored，22.02 秒（`target/06-native-blocker-green.log`），文件树右键菜单和插件状态弹窗下拖入均不创建文件、不改源码、不移动选区。前述 7 项加此项均通过，未声称一次 8 项组执行。
- `cargo test -p editor-app image_offer_queue -- --nocapture` 1 passed（`target/06-queue-final.log`），证明拒绝和关闭释放准备／队列租约。
- 既有格式工具栏组 2 passed（`target/06-toolbar-regression.log`，62.33 秒），源码浮层／SVG 工具栏组 2 passed（`target/06-overlay-regression.log`，138.44 秒）；均显式 `--ignored --test-threads=1`，保留 SDK dialog、状态弹窗及公开 canvas 栅格路径。
- `cargo fmt --check`、独立 Markdown／示例 rustfmt、`cargo test --workspace --exclude editor-app`、`cargo check --workspace` 通过（`target/06-{fmt,workspace-tests,workspace-check}-final.log`）；普通 workspace 67 passed / 93 ignored，35 个结果组。真实 WASM 与原生 ignored 项按上文显式运行，不计入普通套件通过数。
- 独立 Standards 与 Spec 审查最后均 0 remaining findings；junction 保存、输入撤回、忙时提示及浮层阻挡的修正和可观察回归如上。
- 最终 `cargo build -p editor-app`、`./scripts/verify-plugin-sdk.ps1` 通过（`target/06-sdk-{build,verification}-final.log`）。独立仓库外示例完成 SDK 导出、损坏修复及 WASM 构建，最终摘要 `592bd51824e94f5a79f06eb75664ad7f9cb27033bee7786c265e86a3c682a988`。随后经 `./scripts/build-plugins.ps1 -HostExe target/debug/editor-app.exe -Packages markdown` 重建实际 `0.6.0` ZIP（`target/06-package-final.log`），完整访客再跑 28 passed / 0 ignored（`target/06-guest-sdk-final.log`）。
- 最终独立 SDK ZIP 的图片输入／名称冲突重试 smoke：`cargo test -p plugin-runtime --test editor_images image_input_metadata -- --ignored --nocapture --test-threads=1`，1 passed / 0 ignored，16.79 秒（`target/06-sdk-image-smoke.log`）。

## 交付与限制

本单主验收是 Windows 原生自动交互与隔离文件夹。现行编辑器只打开已有路径文档，没有另建 Untitled 产品流程；保存提示验收采用已打开、磁盘文件随后缺失的文档，取消不落盘，确认经 DocumentSession 保存再导入。GIF／WebP 仅第一帧，与 05 一致。完整落盘文件不会随文本 Undo、插件停用或失败清理而删除。保留已有 linker 与未使用 API 警告。任务勾选、导航、代码块高亮与同步滚动继续由 07–10 实施。

本单提交／推送／关闭读回将在交付后追加；#26 不修改。
