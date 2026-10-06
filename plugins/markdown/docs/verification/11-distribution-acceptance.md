# 11 — 默认交付与完整组合验收

状态：已完成，实现、验收、独立审查、普通提交、推送及 tracker 关闭均已读回。固定审查基线 `67331885717aa80a0366d029a56954a16ed5cc2b`；Markdown `0.11.0`、protocol 7、公开能力 1.x，独立 SDK 示例 `0.15.8`。2026-10-04 最终独立 GET 读回 #27–#37 均为 `closed / completed`；父方案 #26 保持 open。交付提交与时间见下文。

## 本单行为

正式打包脚本交付编辑器和八个独立插件包；`bundle-defaults.json` 声明可首次提供的包、哈希及扩展名。宿主按通用索引查找并检查实际 ZIP，不包含 Markdown ID、语言名或扩展名专属分支。索引和包均有读取预算，规范化路径后拒绝越界、设备路径和备用数据流。权限确认持有已检查的不可变包，安装沿公开 Manager 正常生命周期进行。

Manager 完成恢复后才允许首次提供；受信任工作区的当前文件触发主窗口原生权限确认。未确认不启动访客、图片读取或写入。源码或预览提供者已有安装时保留其选择，包括明确禁用的提供者。拒绝、禁用、卸载和两种数据清理策略均在私有数据中保留选择，新版 ZIP、工作区切换和重启不覆盖；只有自动撤销的尚未决定请求允许以后重新提供。

信任撤销、文件身份变化、关闭和窗口撤销即时封闭请求。准备中的声明式资源包及 WASM 都通过 `InstallControl` 最终检查拒绝迟到安装；加载失败的候选释放引用。损坏索引、哈希或私有选择记录按失败处理，主窗口现有错误指示器及状态弹窗显示原因。安装权限、操作名和说明补齐中英文资源。

## 回归与修正证据

- 公开卸载后重启记忆先失败，`11-bundle-choice-red.log` 0/1，随后针对性用例通过；最终公开策略组 `11-bundle-policy-green.log` 5/5、0 ignored，0.15 s。覆盖拒绝与新版包、禁用恢复、卸载保留／删除数据、损坏选择记录、工作区与信任限制、可恢复 offer。
- 实际生产 actor 在资源包 Preparing 阶段仅撤销请求却仍安装，`11-resource-withdraw-red.log` 0/1，0.08 s；增加统一取消 guard 后通用 helper／actor 组 `11-generic-bundle-green.log` 7/7、0 ignored，0.22 s。没有测试专用宿主安装接口。
- 首次确认的重复 entity 借用、损坏包错误丢失、同帧 A→B→A 留存已取消请求均由真实原生回归暴露并修复。`11-native-distribution-green.log` 9/9、0 ignored，133.52 s；验证真实 Base 权限控件、安装后源码 grammar／工具栏／预览、拒绝后新版 ZIP、受限与撤销信任、切页／关闭及迟到结果、替代语言、独立 opaque SDK 预览及卸载。
- 主窗口错误原因的可见性追加真实失败：`11-first-use-visible-error-red.log` 0/1，0.19 s。通过原有通用状态指示器和弹窗补齐显示，以上最终九项包含点击指示器后的实际详情控件。
- 独立审查随后发现初始化失败携带尚未注册的插件 ID，也没有可见错误。新增合法空组件（缺 SDK exports），真实后台 actor 经原生确认后准备失败：`11-first-use-prepare-error-red.log` 0/1，0.26 s，准确命中主窗口缺少指示器。状态按 ID 和完整消息与 entry 错误去重后 `11-first-use-prepare-error-green.log` 1/1、0 ignored，0.27 s；检查实际 registry、entries、views、processes 全空，再真实点击状态弹窗详情，没有伪造 OperationStatus 或特殊测试宿主接口。
- 独立 SDK 预览在启用及明确禁用后重启均保留，`11-native-alternative-preview.log` 1/1，60.39 s；实际预览可见／撤销，不安装默认 Markdown，也不把替代者记成用户拒绝默认。
- 组合输入夹具的 preedit 紧贴围栏造成后续正文成为代码，不属于生产滚动缺陷。改用真实方向键和输入空行保留围栏后，`11-native-combination-green.log` 1/1，46.49 s：单次格式 Undo、任务写回、PNG 同级保存及引用 Undo／Redo、中文 marked composition、未保存内容、深浅主题、20 px 字体、960 × 700 窗口、按实际可见段落反向同步、相对链接打开及返回未保存页、停用撤销全部控件。
- 第一次完整 Markdown 原生组实际为 58 passed、2 failed、0 ignored，1721.18 s，`11-final-markdown-native.log`。旧预览回归错误地要求缓存实体必须销毁；版本门禁实际已隐藏旧树。改验原生标题／工具栏消失、缓存只属于重开后的身份、文本保留及合法重发恢复，完整场景 `11-stale-publication-native-green.log` 1/1，17.03 s，没有为满足内部缓存断言修改生产代码。
- 另一项为真实组合缺陷：相对标题跳转后，初次 Source 布局自动定位覆盖了 Reveal，使标题仍在 pane 外。新增 `link_navigation::synchronized` 沿真实原生链接、公开 owned request 和既有宿主队列执行，交换真实 Opened 回执与 Preview 的到达顺序，并覆盖 EOF clamp／重排；另一场景在旧源码仍有效时要求排队 Locate 取消，再重放同一个 handle 验证不能移动旧预览。旧包 `11-navigation-sync-native-red.log` 0/2，42.26 s，分别准确失败于实际标题不可见和未取消的 Locate。
- 访客使用已有接口取消旧自动定位；只有标题／fragment 导航保留预览优先权，实际预览几何只带动 Source，Unit 回执不代表完成绘制。源侧真实滚动可重新接管，失败／编辑／关闭／切换撤销旧身份。九项新增纯回归与原单测合计 64/64、0 ignored，`11-navigation-sync-guest-green.log`；实际公开 SDK 构建 WASM 后，原八项导航及新取消回归通过。该轮共 9/10、236.81 s，`11-navigation-sync-native-green.log`，唯一失败是新增测试要求缩小窗口后末尾标题永久可见；独立审查按总方案 §5／M07 与 §7／M13 确认这超出批准的内容块同步行为。
- 保留两种到达顺序的首次标题可见断言和中段标题的缩窗断言；末尾缩窗改验实际最靠近预览顶部的块、该夹具短段落的范围保持、对应 Source 真实可见行相交，内存／磁盘文本不变。未为固定末尾标题改生产行为，最终新增原生组 `11-navigation-handoff-native-green.log` 2/2、0 ignored，51.11 s。随后最终发行包的完整 62 项全部通过。

日志均保存在工作区未跟踪的 `target/`；编译失败、旧包的真实回归失败与过约束夹具失败不计通过。最终结果以下列完整组和实际产物为准。

## 最终包与检查

正式打包第一轮成功，八包均为 protocol 7，Markdown 的 15 个资源逐一与源码 SHA-256 一致。首次初始化失败显示修正后重新打包 129.38 s；导航与滚动交接修正后再运行完整正式脚本成功，85.08 s，`11-delivery-formal-package.log`。最终 `dist/editor/editor-app.exe` 与 `dist/editor/plugins/` 为交付产物，同一最终 ZIP 复制到 `dist/plugins/` 供完整原生验收读取。

最终 Markdown ZIP SHA-256：`f3e0950a9a661f805d81fa79d3ce042746f742531b46eef70a5d943765234431`。最终发行 EXE SHA-256：`3352654888e01c0b201f161540eecaf4db4826fd1135403659a37ae03f00d01f`。索引 SHA-256 与实际 ZIP 完全匹配，八包均为 protocol 7、Markdown 15 个资源分别与源码／实际编译产物相同，`11-final-artifact-audit.log`；版本／能力／权限及清单另存 `11-final-artifacts.json`、`11-formal-zip-inventory.json`、`11-markdown-resources.json`。ZIP 不附带 SDK 源码。

| 实际命令 | 结果与日志 |
| --- | --- |
| `./scripts/verify-plugin-sdk.ps1 -HostExe ./target/debug/editor-app.exe` | 仓库外 SDK 导出、损坏后修复及独立 WASM 构建通过，56.27 s；`11-final-sdk-verification.log` |
| 宿主 `--plugin-cargo plugins/markdown/Cargo.toml test --lib` | 导航修正后 64 passed、0 ignored；`11-navigation-sync-guest-green.log` |
| `cargo test -p plugin-protocol` | 28 passed、0 ignored；`11-final-protocol-tests.log` |
| `cargo test --workspace --exclude editor-app` | 80 passed、108 ignored、39 个结果组；`11-final-workspace-tests.log` |
| `cargo test -p plugin-runtime --test sdk_distribution -- --ignored --test-threads=1` | 真实仓库外包 1 passed、0 ignored，18.51 s；`11-final-sdk-smoke.log` |
| `cargo test -p plugin-runtime --test editor_viewport -- --ignored --test-threads=1` | 同一独立包 3 passed、0 ignored，153.31 s；`11-final-runtime-viewport.log` |
| `./scripts/package-editor.ps1` | 导航交接修正后正式 EXE 与八包通过，85.08 s；`11-delivery-formal-package.log` |
| `cargo build -p editor-app` | 最后修正后通过，16.05 s；`11-final-host-build.log` |
| workspace、Markdown 与 capability-example 的 `cargo fmt … --check` | 最终冻结后均通过；`11-final-source-checks.log` |
| `cargo check --workspace` | 最终冻结后通过，1.58 s；`11-final-source-checks.log` |
| `cargo test -p editor-app -- --test-threads=1` | 最后新增原生回归后 230 passed、86 ignored，128.40 s；`11-final-editor-app-tests-green.log` |
| `cargo test -p editor-app extensions::worker::worker_tests -- --ignored --test-threads=1` | 同一真实独立 SDK 包，2 passed、0 ignored，112.71 s；`11-final-worker-lifecycle.log` |
| `cargo run -p editor-app --example svg_preview_render -- dist/editor/plugins/svg.zip plugins/svg/examples/gear.svg target/11-svg-qa.png` | 真实 WASM 与同一原生 renderer 的颜色、透明洞、半透明和棋盘层检查通过；`11-final-svg-diagnostic.log` |
| `cargo test -p editor-app extensions::markdown_tests::synchronized_scroll -- --ignored --test-threads=1` | 导航交接修正后 6 passed、0 ignored，162.58 s；`11-navigation-ordinary-scroll-native-green.log` |
| `cargo test -p editor-app extensions::markdown_tests -- --ignored --test-threads=1` | 正式发行 ZIP 完整组 62 passed、0 failed、0 ignored，1656.53 s；`11-final-markdown-native-green.log` |

SDK 摘要为 `cc5c5f132b293e67927196f5fa7e12d062108ae96bd9addc0ad63abd76049dc8`，与 10 的公开契约相同，本单未增加 SDK 字段。独立构建出来的同一个 ZIP 复制到 SDK 和 runtime 测试目录，两份 SHA-256 均为 `b9c2c83de5d69a1e03870ced35bd0bb6a328692f8cf8fb13c770abb1b8f9c3fa`。原生 60 项完整组暴露上述组合问题；导航修复只使用已有公开能力，修正后重新构建发行包并通过包含两项新增回归的完整组。

最终完整组从 `2026-10-04T08:28:08Z` 执行至 `08:55:46Z`。开始、结束及正式交付 ZIP 三次 SHA-256 均为上述 `f3e095…34431`，退出码 0，见 `11-final-native-artifact-binding.json`；验收期间没有重新生成或替换 ZIP。针对性组与最终完整组分别记录，没有把 skipped 测试计入 62 passed。

初次完整 editor-app 命令编译 SVG 诊断示例时出现五条 E0433：04／05 共享图片渲染器拆分后，独立示例缺少 `bitmap`／`svg` 兄弟模块引用。仅补充已有模块的 `#[path]` 声明与原因注释，保持同一渲染器和分配防护，再运行上述完整命令通过；没有绕过示例编译，也没有为满足测试新增宿主业务分支。

## M01–M16 对照

下表全部已由最终正式包的 62 项原生组及独立契约、生产 actor 与仓库门禁覆盖；组合场景只是其中一项。前序错误边界及独立契约记录见 [01](01-language-package.md)、[02](02-native-preview.md)、[03](03-view-modes.md)、[04](04-format-toolbar.md)、[05](05-image-preview.md)、[06](06-paste-drop-images.md)、[07](07-task-checkboxes.md)、[08](08-link-navigation.md)、[09](09-code-block-highlighting.md)、[10](10-synchronized-scroll.md)。

| 矩阵 | 可观察行为与实际接缝 |
| --- | --- |
| M01 | `distribution` 原生首次权限确认、信任／取消／新版包／禁用及替代者恢复；生产 actor 真实 prepare 失败和资源包撤销 |
| M02 | 实际 WASM grammar 热安装／撤销；`delivered_markdown_preview_tracks…` 及代码块场景的 CommonMark／GFM 预览 |
| M03 | `format_toolbar`、`range_edits` 和组合场景：十三种常用命令、模板／选区、单次 Undo／Redo |
| M04 | `modes`：真实底栏三图标、短分隔线、默认左源码右预览及拖动分割线，无空白侧 |
| M05 | `modes`、`synchronized_scroll`：切文档、重开、独立工作区的三态与同步开关记忆 |
| M06 | 未保存输入、撤销／重做、磁盘重载；source 版本、关闭后新身份及迟到请求拒绝，各原生模块都有版本边界 |
| M07 | `synchronized_scroll` 与组合：两侧真实滚轮、可见块对应、图片／表格／换行／窗口／字体重排、关闭、held pointer 及回执无反馈 |
| M08 | `image_preview`：本地、获授权 HTTP(S)、alt／原因、限制、权限及生命周期；不以原始 HTML 执行替代 |
| M09 | `image_import`：真实剪贴板／拖入、实际 PNG/JPEG/GIF/WebP、同级 img 序列、已有文件、先保存及单次 Undo 保留文件 |
| M10 | `image_import::safety`、图片读取与导航安全：失败收据、名字竞态、路径／符号链接边界，拒绝越权与覆盖 |
| M11 | `task_checkboxes`、`task_safety`：嵌套与空白标记、可见勾选、单字节修改、原选区、Undo、旧手势／版本／IME 拒绝 |
| M12 | `code_highlighting`：真实 novel 提供者、未知语言回退、用户选择、停用／重启与 epoch，主题变化不改源码 |
| M13 | `link_navigation`、`navigation_safety`：中文锚点、相对 Markdown 与片段、原生键盘、图片外链、单次解码、受控浏览器及 Windows junction |
| M14 | 逐模块切页／关闭／停用／卸载／替换，旧请求无跨文档写入；首次提供取消与 prepare 失败不留下注册、面板或进程 |
| M15 | 各模块的深浅主题、双语、Base 焦点与键盘、真实 marked composition；组合场景检查 20 px 字体与 960 × 700 布局 |
| M16 | 仓库外 SDK 同一真实 ZIP、公开 runtime 独立能力夹具、opaque 扩展的首次原生预览；通用宿主没有 Markdown ID／扩展名分支 |

## 独立审查

2026-10-04，独立 Standards 与 Spec 代理分别从固定基线 `67331885717aa80a0366d029a56954a16ed5cc2b` 审查本单 30 个跟踪差异块与 14 个新增文件，并读取最终完整原生日志、测试前后产物绑定及 SDK／资源／仓库门禁证据。最终签收：硬规范 0 项未解决问题，Fowler 0 项发现；Spec 0 项未解决问题，无要求缺失、部分实现、范围蔓延或遗留错误实现。

两条轴均确认正式包的 62 passed、0 failed、0 ignored 与完整哈希一致，末尾标题缩窗断言已按批准的内容块同步行为修正。审查代理只读检查，未运行 Cargo、执行 Git 或修改文件；Git 交付与 issue 关闭由主代理随后执行并单独读回。

## Git 与 tracker 交付

本单普通提交为 [`6819d221a7253a8755947784a42fcf82452f85e6`](https://github.com/T-miracle/Editor/commit/6819d221a7253a8755947784a42fcf82452f85e6)，提交信息 `feat(markdown): bundle support and verify the complete editing workflow`。`git push origin codex/markdown-plugin` 成功，随后 `git ls-remote --heads origin refs/heads/codex/markdown-plugin` 在 `2026-10-04T09:06:39Z` 独立读回同一完整 SHA；证据为 `target/11-feature-push-readback.json`。

核对 [#37](https://github.com/T-miracle/Editor/issues/37) 的数据库 ID `5690699674` 后，仅将该工单设为 `closed / completed`，关闭时间 `2026-10-04T09:07:27Z`。随后对 #26–#37 分别发起独立 GET，最终读回时间 `09:07:35Z`：全部 11 张工单 #27–#37 均已完成；[父方案 #26](https://github.com/T-miracle/Editor/issues/26) 仍为 open，更新时间仍为 `2026-10-03T14:51:53Z`。证据为 `target/11-tracker-final-readback.json`。未修改父方案、强推、重写历史或发布 Release。

该提交仅包含本单明确归属的 44 个文件；工作区既有 `docs/agents/` 改动、根 `AGENTS.md`、项目资料和其他归档副本均未提交。关闭读回后更新总方案、工单和验收索引，纯文档收尾再次检查链接与 diff。

## 平台与交付限制

Windows 原生 GPUI 测试执行真实控件、键盘、焦点、布局及输入接缝；中文 IME 通过真实 `EntityInputHandler` marked composition 验证，没有宣称完成特定 Windows 输入法面板的人工操作。HTTP／浏览器在既有受控外部接缝验证，不依赖公网稳定性。保留既有 LNK4217 与未使用 API 警告，跳过测试单列，不计通过。本单不发布 GitHub Release，也不修改或关闭父方案。
