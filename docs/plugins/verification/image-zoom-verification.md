# Image 滚轮缩放与 SVG 最小初始尺寸

日期：2026-10-06。按用户补充要求，PNG、JPEG、GIF、WebP 允许滚轮缩放，
SVG 初次显示的最长边至少 240px；区域容纳不下时仍完整等比适配。

## 实现边界

原生 `FileImage` 控件直接响应滚轮，默认尺寸策略继续由插件声明。
首次滚轮从当前实际适配比例开始；手动比例固定，窗口尺寸改变时从当帧视口计算中心和裁剪。
切换文件版本或删除图片节点清除缩放意图；旧 revision、失效场景、禁用节点及遮挡场景
不接受旧滚轮回调。不改变文件读取权限、解码预算、只读属性或文件内容。
该行为对所有 FileImage 提供者通用，不增加 Image/SVG ID 分支。

Image 0.5.1 同步 `manifest.json`、`plugin.toml` 与访客 Cargo 版本。
SVG 默认比例为 `max(1,240/max(width,height))` 再按可见区域及既有绘图限额缩小。
240 是最长边的初始最小值，手动 1:1 和滚轮缩小不受该默认值约束。
英文和中文公开原生 UI 契约、插件 README 已同步。

## 实际检查

- `cargo test -p editor-app file_image -- --test-threads=1`：2 项通过。原生输入验证放大、缩小、非中心指针、窗口缩放后保持绝对比例/中心及更换文件清除缩放。
- 宿主 `--plugin-cargo plugins/svg/Cargo.toml test --lib`：7 项通过，包括 100×50 SVG 放大到 240×120、小视口等比缩小、窗口恢复及手动 1:1 保留。
- 宿主 `--plugin-cargo plugins/svg/Cargo.toml check`：通过。
- `./scripts/build-plugins.ps1 -HostExe ./target/debug/editor-app.exe -Packages svg`：构建 Image 0.5.1 与更新本地发现清单。
- `cargo run -p plugin-runtime --example svg_preview_smoke -- dist/plugins/svg.zip`：实际 WASM 默认尺寸、四个按钮、滚轮居中、内容更新及热更新通过。
- `cargo test -p editor-app image_resize_burst -- --ignored --test-threads=1`：实际包尺寸事件合并与滚轮顺序边界通过；新初始尺寸对应放大宽度 268.8。
- `cargo test -p editor-app image_package_draws -- --ignored --test-threads=1`：真实 PNG/JPEG/GIF/WebP 原生文件容器回归通过。
- `cargo fmt --check`、`cargo test --workspace --exclude editor-app`、`cargo check --workspace`：通过。其他 ignored 测试未计为通过。
- `cargo build -p editor-app` 后 `./scripts/verify-plugin-sdk.ps1`：公开 SDK 导出、损坏修复及仓库外独立访客构建通过。
- `npm test`（`website`）：17 项通过。
- 修复版 release 构建：通过，产物 `dist/image-zoom/editor-app.exe`，附带 Image 0.5.1 安装包。

原生测试使用进程级 TEMP/TMP 指向 `target/native-test-temp`，SDK 分发测试需要仓库外目录，
使用 `C:/Users/Tmiracle/.codex/tmp/image-sdk`；未修改全局环境。
曾同时重链接仍在运行的测试 EXE，Windows 返回 LNK1104；待前一测试结束后顺序重跑通过。
访客格式检查直接使用 rustfmt，宿主 plugin-cargo 入口仅支持 build/check/test。

没有操作用户正在使用的窗口或替换其锁定 EXE；需关闭旧窗口后启动新主程序，
在插件管理中更新 Image 0.5.1。未自动安装、提交、推送或发布版本；未声明人工像素/帧率验收。
