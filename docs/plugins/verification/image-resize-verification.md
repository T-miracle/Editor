# Image 预览窗口尺寸回放修复

日期：2026-10-06。范围：用户报告的窗口缩放后工具栏及 SVG 图片缓慢移动。

## 复现与修复

Image 使用现有 `plugins/svg`、稳定 ID `svg`，本次不改插件领域行为或包版本。
旧宿主将每一个画布 Resize 事件串行执行并栅格化、发布。窗口拖动产生积压后，
界面继续显示已经过期的位置，表现为缓慢过渡；没有添加或移除位置动画。

真实 Image WASM 通过公开 Manager 安装后进入实际宿主 Worker，连续发送 300 个尺寸，
从 601 到 900。修复前原生发布边界仍出现宽度 601，回归断言失败。
修复后连续 Resize 只保留最新尺寸；插件、面板、实例 epoch、UI revision、节点必须一致。
点击、滚轮、焦点、关闭及其他工作消息是不可跨越的顺序边界，已读边界保留到下一轮。
每轮最多合并 1024 个事件，持续生产者不能无限占用工作线程。

最终尺寸为 900×500 时图片中心是 (450,266)，保留 32px 工具栏。
两次尺寸变化间插入滚轮后，最终 1000×550 的中心是 (500,291)，图片宽度从 200
变为 224，证明缩放输入没有被几何合并吞掉。宿主不识别 Image/SVG 的业务 ID。

## 实际检查

系统默认临时目录首次返回 `PermissionDenied`，后续命令使用进程级 TEMP/TMP
指向仓库 `target/native-test-temp`，不修改用户全局环境或真实安装数据。

- `./scripts/build-plugins.ps1 -HostExe ./target/debug/editor-app.exe -Packages svg`：构建当前 Image 0.5.0 测试包。
- `cargo test -p editor-app image_resize_burst -- --ignored --nocapture`：先复现旧宽度 601，修复后通过；包含居中与滚轮边界检查。
- `cargo test -p editor-app preview_tests -- --test-threads=1`：1 项通过，SVG 未保存编辑、分栏与交互回归。
- `cargo test -p editor-app image_package_draws -- --ignored --test-threads=1`：1 项通过，真实 PNG/JPEG/GIF/WebP 原生绘制、错误、重试与切回文本。
- `cargo test -p editor-app worker::worker_tests -- --test-threads=1`：2 项通过；另 3 项依赖 capability-example 的 ignored 测试未执行、不计为通过。
- `cargo fmt --check`：通过。
- `cargo test --workspace --exclude editor-app`：默认测试通过；其他 ignored 测试未计为通过。
- `cargo check --workspace`：通过。
- 本次涉及文件的 `git diff --check`：通过；全工作区检查另外报告原有 `README.md:8` 行尾空白，本次未修改该文件。
- `cargo rustc -p editor-app --release --bin editor-app -- -o C:/Projects/RustProjects/Editor/target/release/editor-app-image-resize.exe`：构建通过。

## 交付与限制

当前 `target/release/editor-app.exe` 正在运行，未结束用户进程或替换其文件。
修复版位于 `dist/image-resize/editor-app.exe`，同目录携带当前本地插件包。
需要关闭旧窗口后启动修复版；插件不需要重新安装。
本轮验证了实际 WASM 工作线程的绘图发布及原生 GPUI 输入/布局回归，
未对用户正在运行的窗口做人工拖动、录屏或宣称像素/帧率验收。
未提交、推送、关闭议题或发布版本。
