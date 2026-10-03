# 05 — 图片预览验证记录

状态：验收通过，Git 交付及 issue 关闭另读回记录。基线 `33633b1`；Markdown 包 `0.5.0`，独立公共 SDK 示例 `0.15.4`。只验证工单 05，后续粘贴、拖入与滚动同步尚待对应工单。

## 行为与公开契约

- `ui.images 1.0` 的 `Kind::Image { source, alt }` 使用同一原生 UI 树，要求绑定 `Document.source`，不将图片大字节传进 WASM JSON。所有资源绑定实例、面板、文档版本、节点与 URI。
- 本地地址相对文档目录解析，百分号解码后检查设备路径、备用数据流及规范化、符号链接后的工作区边界；需 `workspace.read`。HTTP(S) 需 `network.images`，禁止凭据、自动跳转和环境代理。
- 全局最多 8 个后台生产者，单图编码至多 8 MiB、管理器驻留编码至多 64 MiB、请求期限 30 秒。取消消费者立即断开，有限的底层 IO 可能稍后结束，不能发布到新文档。
- 原生 worker 解码 PNG、JPEG、GIF/WebP 第一帧与 UTF-8 SVG；单轴至多 4096 像素、当前解码缓存至多 64 MiB。SVG 禁止 SVGZ、DTD 实体及外部／嵌入图片解析器访问；XML 至多 512 KiB、10,000 节点／展开引用与 128 层。滤镜、图案、蒙版及剪裁的临时像素计入分配前配额；文字使用共享系统字体。原生视图仅使用受控返回的 `RenderImage`，没有环境路径或 URL 加载器。
- 独立及段落内图片、引用、列表、GFM 表格保留周围富文本。空或超过 4096 字节的引用只替换该图片为双语原因，原始 HTML 与代码不触发加载。
- 图片成功改变原生布局；宽图随预览栏宽度保持比例。失败展示作者替代文字和双语原因。

## 已执行

- 解码测试 `cargo test -p editor-app ui::plugin::bitmap::tests -- --nocapture`：先 1 passed / 1 failed，再 2 passed；记录 `target/05-decoder-{red,green}.log`。
- 访客 `./target/debug/editor-app.exe --plugin-cargo plugins/markdown/Cargo.toml test preview::tests -- --nocapture`：6 个行为 RED；完整访客套件随后 23 passed、0 ignored，记录 `target/05-guest-{red,green}.log`。
- 公共 SDK 首轮 `cargo build -p editor-app` 与 `./scripts/verify-plugin-sdk.ps1` 通过：示例在仓库外由内嵌 SDK 编译为 `0.15.4`，实际 ZIP 位于 `target/plugin-api-test/capability-example.zip`。新增 SDK 测试文件导出与最终快照另做最终检查。
- `./target/debug/deps/ui_images-cc985cf5f462e605.exe --ignored --nocapture --test-threads=1`：14 passed、0 failed、0 ignored，356.17 秒；记录 `target/image-contract-logs/all-resources-green.log`。覆盖真实独立 WASM 包的能力协商、文档相对路径、百分号与 Windows junction、权限、HTTP 状态／跳转／凭据、大小写 scheme、旧 revision、同 revision 另存为、关闭、请求中停用／新 incarnation、工作区、真实 30 秒超时、两个管理器共用 8 生产者与 64 MiB 编码预算。首轮并发测试曾因未驱动 queued jobs 的 Manager 心跳而等待失败，修复测试驱动后完整重跑；原失败日志保留。
- 字体像素与重复配额解码计数均先 RED 再 GREEN；`cargo test -p editor-app ui::plugin:: -- --nocapture` 24 passed、0 ignored，记录 `target/05-image-decoding-green.log`。SVGZ、实体、XML 密度／深度、定义展开、临时滤镜／图案、可见文字，以及缓存占满、空闲不重解码、节点退休后恢复均覆盖。
- `cargo test -p editor-app extensions::markdown_tests -- --ignored --nocapture`：当前 9 passed / 1 failed，记录 `target/05-native-final.log`。三个真实图片原生用例及前序模式、工具栏、事务、SDK 覆盖通过；失败是旧图片文字占位断言，与本单原生图片节点语义冲突，已改为图片声明与空地址局部双语提示，需最终重跑。
- 引用解析复审要求 XML 预检与 usvg 的 IRI 空白、xlink 优先级及重复 id first-wins 一致；在解析接缝实际 RED `[false, false, false]`，记录 `target/05-reference-parser-red.log`，修复后的 GREEN 与滤镜 CPU、GPU 纹理生命周期见最终收尾证据。

## 最终收尾证据

- SVG 转换前配额进一步计入文字字符、路径属性载荷及图案／渐变／滤镜等定义的潜在复制成本；样式和父节点继承按保守上界处理。噪声 octaves 至多 8、卷积 order 至多 15；转换后的效果共用 16 Mi 采样工作额度，按实际区域面积乘迭代／核规模收费，并限制缩放后 morphology 半径、blur／shadow sigma 和偏移，防止上游整数溢出。复杂 SVG 可局部返回超额，不运行危险滤镜来制造测试。
- `cargo test -p editor-app ui::plugin:: -- --nocapture` 最终 31 passed、0 failed、0 ignored，记录 `target/05-decoding-final.log`。引用语义、路径／文字载荷、父级及 CSS paint 复用、效果参数／工作量均在对应安全解析或分配预检接缝 RED → GREEN。实际 RED 日志 `05-effects-work-red.log`（2 failed / 1 passed）、`05-blur-work-red.log`、`05-geometry-payload-red.log`、`05-paint-reuse-red.log` 保留。
- `cargo test -p editor-app delivered_markdown_image_atlas -- --ignored --nocapture --test-threads=1`：3 passed、0 ignored，57.42 秒，记录 `target/05-native-atlas-green.log`。真实 ZIP 绘制后的 atlas 检查先证明停用未释放、最小回收会误伤共享投影，再以 App 内有效 ImageId 投影使用计数修复；版本换代旧纹理清理、仍有效的共享投影保留、最后一个投影退休才清理。计数不依赖 worker／测试持有的 Arc 数量，回收显式携带当前 Window。新控件原地替换回收另由上述原生层单元套件覆盖。
- 旧图片文字断言迁移后，`cargo test -p editor-app delivered_markdown_preview_tracks_unsaved_native_edits_and_reclaims_split -- --ignored --nocapture`：1 passed，记录 `target/05-preview-regression-green.log`。与前述原批 9 passed 合计前序及图片 10 项通过，未将旧批次失败伪报为成功。
- `cargo test -p editor-app svg_preview_follows_open_documents -- --nocapture`：1 passed，记录 `target/05-svg-regression-green.log`，现有 SVG 打开、未保存编辑及退休行为保留。
- `cargo fmt --check`、`cargo test --workspace --exclude editor-app`、`cargo check --workspace` 均通过，日志 `target/05-{fmt,workspace-tests,workspace-check}-final.log`。普通 workspace 61 passed / 83 ignored；ignored 项不计通过，本单真实资源 14 项及原生 ignored 用例已按上文显式执行。
- 最终代码冻结后的独立 Standards 与 Spec 审查均 0 remaining findings；已修复的审查项及其行为证据如上，审查代理只读、不代替执行者的实际验证。
- SDK 最后说明同步后再次 `cargo build -p editor-app`、`./scripts/verify-plugin-sdk.ps1` 成功，记录 `target/05-sdk-docs-{build,verification}.log`，实际输出 `Verified public SDK export, repair and independent component build.`。最终独立示例使用内嵌 SDK 摘要 `ae1c13ab149d3f47cf60b1472e8846fd1f4a13d0f9a5570a3746c65bc5db136b`。
- 新包 `cargo test -p editor-app extensions::markdown_tests::image_preview -- --ignored --nocapture --test-threads=1`：6 passed、0 ignored，115.70 秒，记录 `target/05-native-images-package-final.log`，覆盖显示／局部失败、比例／宽度、迟到网络和三项纹理回收。
- 独立访客 `cargo fmt --manifest-path plugins/markdown/Cargo.toml --check` 通过。宿主 `--plugin-cargo` 仅支持 build/check/test，最初 fmt 调用被明确拒绝，改用常规 rustfmt 检查；发现的源文件格式差异已整理。随后重新构建 `dist/plugins/markdown.zip`（15 项资源、`0.5.0`）并执行完整访客 23 passed / 0 ignored，日志 `target/05-package-formatted.log`、`target/05-guest-final.log`。最终独立 SDK 新包资源读取 smoke 1 passed / 0 ignored，记录 `target/05-sdk-package-resource-smoke.log`。

## 交付及实际限制

本单为原生 GPUI Windows 自动交互验收与隔离本地／loopback HTTP 测试，未请求真实第三方网站。GIF/WebP 暂仅第一帧，SVG 复杂度配额采取保守上界。系统 DNS 物理调用不能立即中断，但仍位于 8 个已计数 worker 内，消费者超时即失效且迟到解析不再连接。保留已有 linker 与未使用 API 警告。粘贴／拖入图片、任务交互、链接、提供者代码高亮及同步滚动按后续工单继续实施。本单普通提交、推送验证及 #31 completed 读回将在交付后补证据，不关闭父议题。

2026-10-04 交付：普通提交 `9f3670ed0185ac12a85a2c23a49148a8dbf80c2a` 已推送 `origin/codex/markdown-plugin`，`git ls-remote` 核对远端 SHA 完全一致；GitHub #31 PATCH 后独立 GET 读回 `closed / completed`。#26 保持未修改。此交付读回追加到后续工单的文档提交，未改写已推送历史。
