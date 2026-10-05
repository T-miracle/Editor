# Image 插件

安装 `svg.zip` 后，Image 提供 SVG、PNG、JPEG（jpg／jpeg）、GIF 和 WebP 显示，扩展名不区分大小写。保留已有包标识 `svg`、安装范围与私有数据。SVG 使用原生源码编辑与预览；其他格式只显示图片，沿用文件 Tab，不创建文本编辑会话。GIF／WebP 首期显示静态帧。

底栏左侧工具组右边以短竖线分隔，提供“仅编辑、分栏、仅预览”三个 SVG 图标按钮，最近模式按工作区记忆。三个按钮与 Markdown 使用同一公开宿主布局能力。

预览直接使用编辑器内的未保存内容，支持输入、粘贴、撤销及磁盘内容重新加载。源码输入暂时不完整时，右侧显示解析说明；修正后自动恢复预览。

SVG 保持原有颜色和透明度，绘制在白色／灰色棋盘之上。棋盘只覆盖 SVG 底座，周围保持面板背景。

以下缩放按钮和滚轮交互仅用于 SVG；PNG、JPEG、GIF、WebP 目前使用原尺寸／超区自动缩小，不提供手动缩放。

- 在 SVG 预览图片区域滚动鼠标滚轮即可缩放；滚轮与所有缩放按钮均保持图片中心固定在预览画布中央。
- 默认按图片自身尺寸显示；超出预览内容区域时等比缩小至完整可见，小图不自动放大。自动适配会跟随区域尺寸、文件切换和 SVG 画布尺寸变化；SVG 的手动缩放仍允许主动放大。
- 左上角四个 SVG 图标依次为：放大、缩小、以原始尺寸缩放（1:1）、以窗口尺寸缩放。
- 右上角显示当前缩放比例，字体、字号和字重跟随编辑器默认 UI 文字样式；窄面板内比例移至工具栏第二行，避免覆盖图标。
- 以原始尺寸缩放会居中并恢复原尺寸；以窗口尺寸缩放会完整适应当前预览区域，并跟随分割线变化。
- 普通 SVG 支持 1%–3200% 缩放；极大或极小的 SVG 会扩展范围以支持默认尺寸和窗口适应，最终受绘图尺寸上限约束。
- 已放大的图片裁剪在预览面板内；改变分割线宽度会更新预览视口。

0.4.0 使用 `protocol = 7`，协商 `ui.native`、`ui.canvas >=1.1`、`editor.documents`、`editor.files`、`ui.file_images` 与 `editor.presentation`。权限 `editor.read` 用于当前 SVG 内存内容与文件上下文；`workspace.read` 用于受控图片文件资源。更新时新增权限须由用户确认。每帧回传来源身份、路径与 revision，旧版本不会覆盖新内容，同一路径重新打开也使用新身份。普通 Canvas 承载矢量、主题字体和原生指针事件，宿主无需识别插件 ID。插件不申请进程或网络权限。

支持静态 SVG 的路径、颜色、渐变、透明度及嵌入资源；主程序负责原生文字和图像渲染。SVG 外部图片路径不会被读取。单个 SVG 源码上限为 1 MiB。其他格式通过独立文件版本与受控原生图片资源读取，要求 `editor.files`、`ui.file_images` 和 `workspace.read`；编码字节上限 8 MiB，解码尺寸／内存遵守宿主有限预算，超限、损坏和未授权均显示原因。

四个图标源文件位于 `icons/zoom-in.svg`、`icons/zoom-out.svg`、`icons/actual-size.svg`、`icons/fit-window.svg`，同时打包到 ZIP 中。图标颜色随预览面板主题变化。

```powershell
# 主程序提供协议与 WIT 接口缓存。
cargo build -p editor-app
./target/debug/editor-app.exe --plugin-cargo plugins/svg/Cargo.toml test --lib
./scripts/build-plugins.ps1 -HostExe ./target/debug/editor-app.exe
# 通过真实 WASM 组件验证文档同步、缩放、解析恢复和权限。
cargo run -p plugin-runtime --example svg_preview_smoke -- dist/plugins/svg.zip
# 检查真实组件的原生颜色、透明洞和半透明像素，并输出预览图。
cargo run -p editor-app --example svg_preview_render -- dist/plugins/svg.zip plugins/svg/examples/gear.svg target/svg-render.png
# 使用隔离数据启动原生窗口，验证插件加载及正常关闭。
./scripts/svg-startup-smoke.ps1
```
