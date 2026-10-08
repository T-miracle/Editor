# 图标工具栏与首次预览滚动回顶

日期：2026-10-04。目录：`C:/Projects/RustProjects/Editor`。状态：四项需求实现、定向原生验收与本地发行产物已完成。

## 用户要求与实现

- 十三个格式按钮均改用 SVG 图标；六个参考用户提供的 Heading、粗体、斜体、删除线、行内代码、代码块图形，其余按相同风格补齐。全部为 24 × 24 画布、2 px 圆角描边，不依赖字体。源码工具栏保留中英文提示。
- 通用 `ui.icons ^1` 提供 `Node.button_icon` / `.icon(svg)`，以原生按钮渲染，保留 label 的可访问名称、禁用态、焦点与版本化 Click。单个 4 KiB、整篇 64 个；运行时复用既有 XML 几何白名单，拒绝外部资源及活动 SVG。
- 图标按钮 24 px 高；工具栏上下各 2 px，窄分栏按组换行。底栏面板开关通过已有 `icon_light` / `icon_dark` 声明 Toolbar 图标，位于同步滚动右侧；隐藏面板时仍可重新打开。排序按通用面板类型与图标声明处理。
- 插件清单、贡献文件与 Cargo 版本提升至 0.12.0，新增 `ui.icons` 要求，权限不增加。旧宿主应明确拒绝缺失能力，不能静默退回文字按钮。

## 可重复的回顶与修复

真实 Markdown ZIP 经公开 `Manager.install` 与 GPUI 原生窗口打开文件，首次预览绘制后立即输入滚轮 -900 px，保持反向源码 Locate 排队，再通过公开 Theme 通知完成启动配置并继续正常 Worker 发布。旧实现中首段上边从 -826 px 返回 62 px，恰好回到视口顶部 62 px。日志：`target/markdown-icons-first-scroll-startup-red.log`，1 failed / 0 ignored。

根因位于插件 `Scrolling.refresh(false)`：同一源码版本因 locale 更新重建派生 UI 时调用 reset，清除最近手动滚动方向；反向 Locate 未完成，迟到的源码顶部布局由此成为新驱动。修复为取消旧请求并保留驱动视口，只在真正的源文档版本变化时 reset。相同原生回归已转绿：`target/markdown-icons-first-scroll-green.log`，1 passed / 0 ignored，22.22 s。

初始无延迟更新场景没有复现；第二次仅发现 3.5 px 重排误差，不等于用户的回顶现象。最终断言检查滚动仍明显离开顶部，不把像素微差误报为回顶。

## 验证进度

图标工具栏原生 RED：旧包在 1400 × 900 窗口、598 px 源码宽度下工具栏高 77 px，并且没有所需底栏图标，`target/markdown-icons-toolbar-red.log`，2 failed / 0 ignored。

公开协议测试：`cargo test -p plugin-protocol icon_buttons`，2 passed / 0 ignored。覆盖图标字节序列化、可访问名称、版本化 Click、禁用、类型与共享配额。

先 `cargo build -p editor-app`，再 `./scripts/verify-plugin-sdk.ps1 -HostExe target/debug/editor-app.exe`，SDK 导出、损坏修复和仓库外独立 WASM 构建通过。该脚本交付的同一 `capability-example.zip` 用于通用运行时回归：`cargo test -p plugin-runtime --test composable_ui ui_icons -- --ignored --test-threads=1`，2 passed / 0 ignored，185.31 s。一个独立插件身份只协商图标等基础 UI，验证正常图形保留及未协商拒绝；八种不安全或无效 XML 均被公开 Manager 原子拒绝。

Markdown 单元组：`target/release/editor-app.exe --plugin-cargo plugins/markdown/Cargo.toml test --lib`，64 passed / 0 ignored。包括滚动、导航、格式、任务和解析规则；导航放弃未测量的定位时仍恢复普通初始对齐。

正式 ZIP 与原生测试 ZIP 字节相同。实际包 → Manager → GPUI 原生按钮、滚轮与输入窗口，定向 18 passed / 0 ignored：

| 过滤组 | 通过数 | 耗时 | 观察行为 |
| --- | ---: | ---: | --- |
| `icon_toolbar` | 2 | 41.54 s | 13 个 24 × 24 按钮、29 px 单行、两主题、窄宽换行；底栏位置与隐藏后重开 |
| `first_open` | 1 | 28.12 s | 首次绘制后立即滚动，延迟配置与反向定位不回顶 |
| `format_toolbar` | 3 | 124.85 s | 中文选区、13 个空选区模板、键盘、窄分栏、单步 Undo/Redo |
| `synchronized_scroll` | 6 | 187.92 s | 双向接管、偏移回执、图片/表格/换行/分栏重排、撤销与迟到结果 |
| `source_layout` | 2 | 76.37 s | 实际输入区撑满剩余高度、长/空文档、窗口/主题重排、SVG 与普通文档 |
| `modes` | 2 | 53.86 s | Markdown 与 SVG 三态、工作区记忆、文档/选区/撤销保留 |
| `link_navigation::synchronized` | 2 | 55.01 s | 片段定位两种通知顺序、尾部夹紧、导航取消旧同步 |

命令统一为 `cargo test -p editor-app markdown_tests::<过滤组> -- --ignored --test-threads=1`；图标组日志为 `target/markdown-icons-toolbar-green.log`，其余为 `target/markdown-icons-native-<组>.log`。滚动测试按职责移入 `first_open.rs`，原双向滚动模块没有增加新的业务责任。

首次打包尝试被既有 panel SVG 文件头检查拒绝：图标注释在 `<svg>` 前。已将该注释放入根元素，重建正式包后原生图标组通过。没有放宽资源校验来接受错误产物。

所有 14 个新图标已通过现有 resvg 栅格化并检查深浅主题下的 40 px 放大与 14 px 实际尺寸。审阅图为 `target/markdown-toolbar-icons.png`，生成器为同目录诊断文件，不交付为运行时依赖。

公共按钮渲染与全局底栏布局还影响普通 UI，因此追加 editor-app 默认组。仓库门禁结果：

| 命令 | 实际结果 | 日志 |
| --- | --- | --- |
| `cargo fmt --check` | 通过，1.33 s | `target/markdown-icons-fmt.log` |
| `cargo test --workspace --exclude editor-app -- --test-threads=1` | 116 passed / 115 ignored，199.92 s（含构建） | `target/markdown-icons-workspace-tests.log` |
| `cargo check --workspace` | 通过，2.74 s | `target/markdown-icons-workspace-check.log` |
| `cargo test -p editor-app -- --test-threads=1` | 246 passed / 103 ignored，130.61 s | `target/markdown-icons-editor-default.log` |
| `git diff --check`、受影响文档本地链接 | 通过 | `target/markdown-icons-diff-check.log` |

默认组的 ignored 不计作通过；本次涉及的图标安全与上述 18 项原生包测试另行显式执行，均 0 ignored。保留已有 unused 与 Wasmtime/Tree-sitter 链接器告警，没有编译或测试错误。验收限本机 Windows、真实包与 GPUI 测试窗口；没有控制用户的运行窗口，也未重跑此前 0.11.0 的整套 62 项 Markdown 验收或 macOS/Linux。

## 本地发行产物

发行宿主已构建：`cargo build -p editor-app --release`，156.62 s。`target/release/editor-app.exe` 与 `dist/editor/editor-app.exe` 字节一致，长度 94,243,840，SHA-256 `5ae4e2f6fbfacf1eb176379ec0cbfa6066a27461c6d0c591707380d86b811167`。

使用该发行宿主的 SDK 构建：`./scripts/build-plugins.ps1 -Packages markdown -HostExe target/release/editor-app.exe -Output dist/editor/plugins`。`dist/editor/plugins/markdown.zip` 为 0.12.0、29 个条目，包括十三个格式 SVG、Toolbar SVG 与三个既有模式 SVG。SHA-256 `0af51e3c8d43d642ad436835272a608e7fe0ff28c7793451523627bc6e5c7951`；复制到 `dist/plugins/markdown.zip` 后逐字节哈希相同。首次提供索引 `dist/editor/plugins/bundle-defaults.json` 指向同一 SHA，索引本身 SHA-256 `670cf82bc153e79ff93a485e9ce45df22356dcd5e527050081fa4c329a1c5547`。

只读核对用户配置目录时，已安装版本仍为 0.11.1、启用。源码构建不会更新该安装副本；生效步骤是启动上述新 EXE，并从正常插件管理入口使用新 ZIP 更新到 0.12.0。未修改用户 registry、授权、工作区选择或未保存文本。
