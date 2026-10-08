# Image 0.6.0 与通用可视视口验收

日期：2026-10-06。对应[职责决定](../specs/visual-viewport.md)。
此前 0.5.1 的原生图片缩放记录保留为历史；本次将缩放策略移回 Image 插件。

## 交付

- 公开 `ui.viewport 1.0.0`、`VisualViewport`、`ContentTransform`、`ContentSize` 和 `Action::ViewportInput`。
  文件图片、文档资源图片和画布通过同一能力协商、校验和投影，不按插件 ID 分支。
- 移除宿主 `file_image_zoom`、滚轮步长和比例范围。Image 保存自己的比例、自动适配和文件切换重置。
  普通图片每帧使用实际视口居中和裁剪，手动缩放不随窗口变化重置。
- 连续尺寸合并支持新输入信封，保留滚轮增量及输入边界。
  同一文件节点的几何变化保留 revision，防止快速滚轮增量因先前变换发布而被丢弃。
- SVG 棋盘透明底座、居中缩放、240px 初始最小最长边和默认 UI 字体继续有效。
- 包清单、声明和访客 Cargo 版本同步为 0.6.0；英文及中文 SDK 文档、SDK 内嵌源码注册表同步更新。

产物为仓库根下 `dist/image-viewport/editor-app.exe`、`dist/image-viewport/plugins/svg.zip`，
独立插件包也在 `dist/plugins/svg.zip`。发行目录其余包从现有产物复制，未重建或修改其业务实现。
旧主程序仍在运行，构建使用独立 release 输出，未停止用户窗口或修改现有安装数据。

## 实际验证

- `cargo fmt --check`、访客 `rustfmt --check`、`cargo check --workspace`：通过。
- `cargo test --workspace --exclude editor-app --jobs 1`：136 项通过、126 项 ignored；未将 ignored 计为通过。
- `cargo test -p editor-app ui::plugin:: -- --test-threads=1`：51 项通过，含文件图片原生滚轮/尺寸/旧资源回调及画布共用输入。
- 主程序 `--plugin-cargo plugins/svg/Cargo.toml test --lib`：8 项通过；包含公开输入驱动的栅格缩放、同 revision 连续滚轮、尺寸变化保留手动比例及文件切换重置。
- `cargo test -p plugin-runtime --test file_views -- --ignored --test-threads=1`：真实包 2 项通过；两种插件 ID 的资源与缩放行为一致，未协商能力的发布被拒绝。
- `cargo test -p editor-app image_resize_burst -- --ignored --test-threads=1`：1 项通过；停止拖动后不回放中间位置，滚轮边界保留。
- `cargo test -p editor-app image_package_draws_readonly -- --ignored --test-threads=1`：1 项通过；真实 PNG/JPEG/GIF/WebP 原生显示、错误、重试、切换及关闭。
- `cargo build -p editor-app` 后执行 `scripts/verify-plugin-sdk.ps1`：公开 SDK 导出、损坏修复和独立 WASM 构建通过。
- `scripts/build-plugins.ps1 -HostExe ./target/debug/editor-app.exe -Packages svg`：Image 0.6.0 构建及打包通过。
- `npm --prefix website test`：中英文文档、链接和搜索 17 项通过。
- `svg_preview_render`：真实组件与原生栅格化的颜色、透明洞、半透明像素及棋盘层次通过；输出 `target/image-viewport-render.png`，已目视检查。
- `scripts/svg-startup-smoke.ps1 -HostExe ./dist/image-viewport/editor-app.exe`：真实 WASM 的源码同步、工具栏、居中缩放、字体和更新通过；独立原生窗口启动、绘制生命周期及正常关闭通过。

## 限制与检查说明

原生窗口截图接口返回 `capture=False`，不声称获得完整 GPU 窗口截图；上面的 PNG 是独立绘图诊断产物。
macOS/Linux 未实测。其他工单的 ignored 测试未在本任务中扩大运行。
工作区首轮测试发现本次测试夹具的 PathBuf 参数问题，已修正；同轮有 Windows 链接器
`msvcrt.lib` 的环境失败，串行重跑完整非 UI workspace 后通过。
本次变更范围的 `git diff --check` 通过；全工作区检查仍报告已有 `README.md:8` 的行尾空白，未修改无关内容。
未提交、推送或发布版本。使用新能力需启动交付的新宿主并将已安装 Image 更新为 0.6.0。
