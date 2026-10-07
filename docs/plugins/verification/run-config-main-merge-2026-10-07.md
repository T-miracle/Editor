# 运行、调试、构建与插件配置树：主分支集成验收

日期：2026-10-07。用户明确要求“合并至主分支并推送远程”，替代此前仅在功能分支交付的限制。父设计议题 #48、#67 不随本次合并关闭。

## 候选与范围

- 主分支起点：`db53d2d0bcaae7ca3106f83227e8d1ef902215af`，已包含插件 UI 解耦；读回的 `origin/main` 为其祖先 `0ed129d90a8bcbc50ffe9d62f709a45bb6babee7`。
- 功能分支：`Editor-run-debug-build`，交付点 `868b760e90bcf8479cf42e965ae7ea92188e3df8`。保留完整历史，通过普通双父提交合并，不压缩或改写历史。
- 候选工作区：`C:/Users/Tmiracle/.codex/worktrees/run-config-main-merge/Editor`。先在独立、无原工作区改动的候选中解决冲突并验收，再更新主分支。
- 功能分支的未提交 `Cargo.lock` 改动不纳入合并。主工作区原有 Markdown、SVG、增量渲染、图标及文档重归档等未提交修改也不纳入候选。

## 冲突处理与行为接缝

- 保留主分支的文件身份与可选 `TextTab`，运行前保存只访问真实文本会话；图片等只读文件没有保存义务，也不能借用后台编辑器进行调试源定位。未确认的磁盘冲突及失败保存继续阻止启动。
- 保留主分支拆分后的原生插件渲染器，在叶控件接入多行输入及配置表单按钮标识，避免恢复旧版整块渲染器。`ui.tools`、内容颜色和偏好导入与新运行协议共同保留。
- SDK 同时导出主分支的布局、工具栏、偏好契约和功能分支的配置、执行、会话、调试契约。站点是唯一读者正文来源；离线导出删除 Astro 元数据、改写本地链接，并保留正文原有 LF／CRLF。
- 终端与示例包同时包含两边的业务能力，版本分别更新为 `0.12.1`、`0.16.1`。实际 Rust 包为 `0.5.0`，调试器包为 `0.1.0`，Image／SVG 为 `0.5.0`，Markdown 为 `0.12.0`。
- 英中读者文档保留现行文件布局与语言资源契约，补齐新配置、服务和进程版本；已在主分支迁至站点的旧协议正文不重新恢复。

## 验证

完整日志保存在仓库外的本机目录 `C:/Users/Tmiracle/AppData/Local/Temp/editor-run-config-main-merge-20261007`，生成的 SDK、WASM、ZIP、站点及调试器缓存不提交。

| 验证入口 | 结果及边界 |
| --- | --- |
| `cargo fmt --check`、`cargo check --workspace` | 通过；不把编译当作原生行为验收 |
| `cargo test --workspace --exclude editor-app` | 196 passed、0 failed、167 ignored；跳过项不计为通过 |
| `cargo test -p editor-app -- run_ui_tests sdk_export run_file_tabs` | 23 passed、0 failed；覆盖选择器、真实文件保存边界及 SDK 离线导出 |
| `cargo build -p editor-app`、`./scripts/verify-plugin-sdk.ps1` | 通过；导出、缓存修复及仓库外独立组件构建 |
| `./scripts/build-configuration-example.ps1` 及按需构建实际插件 | 通过；当前公开 SDK 构建 Alpha／Beta 和上述真实消费者 |
| 实际包原生批次 | 15 个不同场景通过：13 项插件配置、1 项 Image 原生文件显示、1 项撤销信任后拒绝启动；首跑 12 passed、3 failed，定位后仅复验失败的 3 项，3 passed、0 failed |
| 文档源、链接与 `npm run build` | 9 项源检查、6 项链接检查通过；站点构建与 17 项构建后测试通过 |
| `git diff --cached --check` | 通过 |

首轮普通应用回归为 363 passed、20 failed、141 ignored。失败定位后，移除旧 B3 宿主表单的 14 条失败断言及对应旧表单测试模块：本次确认的插件表单、配置树、事务和故障行为由实际包入口验收，不恢复已被新规格替代的入口。仍适用的选择器、保存冲突和 SDK 用例已修正并通过上述 23 项针对性回归。受限工作区用例补齐真实包的构建前置标记，并改由当前插件模板创建有效配置后撤销信任，再显式验收启动被拒绝。

实际包主命令为 `cargo test -p editor-app --bin editor-app -- plugin_configuration image_package_draws_readonly a_restricted_workspace_refuses_to_launch --ignored --nocapture --test-threads=1`。首轮实际 Cargo 构建／调试场景发现验收进程继承 `CARGO_TARGET_DIR`，产物被写入共享编译缓存而非临时项目的 `target/`；共享缓存中的实际产物与时间戳证实此原因。复验使用 Cargo 的 `--target-dir` 参数复用宿主缓存，并取消该验收进程的环境变量，使子项目使用其正常产物位置；不改变用户或系统的全局环境。修正夹具的受限工作区用例、Cargo 构建与 Shell 执行、真实 CodeLLDB 的断点／调用栈／局部变量／步进／停止均通过，复验三项耗时 195.62 秒。MSVC 库搜索路径也仅在验证进程中配置，解决首次非 UI 测试的 `msvcrt.lib` 环境缺失，不安装或升级工具链。

本次原生验收包含实际 Windows GPUI 控件、公开 WASM 包、真实程序和调试器。此前系统 Microsoft Pinyin 候选窗口、组合提交及 release 布局证据仍保留于 [功能分支终验](run-config-plugin-tree.md)；本次不声称重新运行系统 IME 或重新生成发行包。旧配置继续采用显式、限定工作区的清理命令；合并不自动清理主工作区或用户配置，也不自动更新用户已安装的插件。

## 主工作区与发布核对

主工作区起始 index 为空，原有 272 个变更路径已分别记录存在状态与 SHA256，并保存 220 个现存文件的原始字节和 52 个删除状态。更新前复核与备份完全一致；通过单独备份保护原有修改，恢复后保持未提交状态，不混入本次合并提交。

采用普通推送更新 `origin/main`，最终交付以实际合并提交、其两个父提交，以及推送后远程分支 SHA 的读回为准。远程若先行更新，必须保留其历史再合并，禁止强制推送。
