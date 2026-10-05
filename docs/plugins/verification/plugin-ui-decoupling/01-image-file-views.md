# 工单 01：Image 与文件显示验收

日期：2026-10-06。议题：[#62](https://github.com/T-miracle/Editor/issues/62)。
分支：`codex/plugin-ui-decoupling`；审查起点：`4d4a7b6`。
状态：实现、阶段验证与双轴审查通过，准备推送并核对关闭议题。

## 交付内容

实际 `plugins/svg` 包显示名称改为 **Image 0.4.0**，保留稳定 ID `svg`、
已有安装范围与私有数据。SVG 继续使用原生文本、未保存预览、撤销和手动缩放；
PNG、JPEG、GIF、WebP 只显示受控文件图片，不创建文本会话或提供编辑模式入口。
非文本文件与普通文档共用文件 Tab；无查看器、失效和损坏时保留文件及重试目标。

`editor.files 1.0` 与 `ui.file_images 1.0` 分别提供独立文件版本与当前文件图片节点。
资源沿原有 Manager 权限、规范化边界、有限后台读取和原生解码路径执行。
默认以固有尺寸显示，任一方向超出实际内容区域时等比缩小，小图不自动放大。
文件重载使用流式指纹，不把图片当作 UTF-8 文档，也不缓存第二份可变内容。

## 实际检查

在独立工作区执行：

- `cargo fmt --check` 与 SVG 访客格式检查：通过。
- `cargo test --workspace --exclude editor-app`：默认测试通过。被 ignore 的实际包用例不计为通过。
- `cargo check --workspace`：通过。
- `cargo test -p editor-app empty_canvas -- --test-threads=1`：4 项通过，包含无查看器的非文本 Tab、输入／保存保护、关闭及图片魔数说明文本保留编辑能力。
- `cargo test -p editor-app file_watch`：4 项通过。
- `cargo test -p editor-app preview_tests`：SVG 源码、未保存内容与原生分割回归通过。
- `cargo test -p editor-app language_tests -- --test-threads=1`：5 项通过。
- `cargo test -p editor-app diagnostics -- --test-threads=1`：16 项通过，3 项 ignored 未计入覆盖。
- `cargo test -p editor-app file_image`：尺寸边界回归通过，覆盖原尺寸、刚好容纳、单轴／双轴超出和恢复。
- 通过本分支宿主 `--plugin-cargo plugins/svg/Cargo.toml test --lib`：6 项通过。
- `./scripts/build-plugins.ps1 -HostExe ./target/debug/editor-app.exe -Packages svg`：实际 `dist/plugins/svg.zip` 已构建。
- `cargo test -p plugin-runtime --test file_views -- --ignored`：通过；实际 Image 与另一个包 ID 经公开安装路径验证文件身份、读取、迟到版本拒绝、关闭重开、权限撤销和越界路径拒绝。
- `cargo test -p editor-app image_package_draws -- --ignored`：通过；真实组件的 PNG／JPEG／GIF／WebP 进入原生文件容器，验证绘制、无源码入口、损坏提示、重试入口及切回文本。
- `cargo build -p editor-app` 后运行 `./scripts/verify-plugin-sdk.ps1`：独立临时项目构建、完整导出及损坏缓存修复通过。
- `cargo test -p plugin-runtime --test sdk_distribution -- --ignored`：真实独立 SDK 包安装与运行通过。
- `./scripts/svg-startup-smoke.ps1 -HostExe ./target/debug/editor-app.exe`：实际 SVG 编辑、缩放、解析恢复与热更新检查通过；Windows 原生窗口加载、私有状态保存及正常关闭通过。
- `git diff --check`：通过。

## 已发现与处理

### Standards 审查

以 `4d4a7b6` 为固定起点审查实际提交；初次发现 3 项规范问题及 1 项重复映射。
文件 Tab 模型已移入 `editor/tabs.rs`，恢复 `call()` 的执行预算注释，Image 说明限定 SVG 手动缩放，
两种图片共用错误提示映射。`aa7fb75` 修正复核通过，无剩余阻断。

### Spec 审查

初次发现 2 项 P2：短图片魔数误移除普通文本编辑能力，以及路径校验误拒合法 `#` 文件名。
分别在 `24729d8`、`b001da5` 修正，原生四种文本前缀和真实包 `photo#1.png` 回归均先失败再通过。
修正后的 fmt、非 UI workspace tests 和 workspace check 再次通过；Spec 复核无剩余阻断。

### 环境及范围

原语言测试并发共享全局注册表，混跑时三个用例失效；同一版本串行执行 5 项全部通过。
后续应用集成按串行执行，避免把全局注册表干扰误记为产品行为。

首次原生启动检查使用旧快照文件名，实际状态已成功保存在工作区私有目录的
`state.json`。脚本已改为检查现行作用域目录，修正后通过。
Windows `PrintWindow` 在本机返回 `capture=False`，未生成截图；
不将此记录描述为截图或人工像素验收。原生 GPUI 绘制／输入与实际窗口生命周期分别已验证。

完整可组合布局、提供者选择、两组底栏、共享显示偏好和插件 UI 迁移分别由 02–04 接续，
最终跨插件组合、故障和响应性由 05 总验收。此记录只覆盖工单 01 的单查看器路径。
