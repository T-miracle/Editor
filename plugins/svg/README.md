# Image 插件

安装 `svg.zip` 后，Image 提供 SVG、PNG、JPEG（jpg／jpeg）、GIF 和 WebP 显示，扩展名不区分大小写。保留已有包标识 `svg`、安装范围与私有数据。SVG 使用原生源码编辑与预览；其他格式只显示图片，沿用文件 Tab，不创建文本编辑会话。GIF／WebP 首期显示静态帧。

Image 0.6.0 通过公开 `editor.layout` 与 `ui.tools` 声明自己的布局和功能。窗口按钮组右侧以短竖线分隔的工具组提供 SVG 的“仅编辑、分栏、仅预览”，同工作区 SVG 共享模式。插件私有存储保留旧模式，已有新版设置优先；PNG 等没有文本能力的图片不显示源码模式按钮，也不应用 SVG 源码偏好。

普通图片使用所有插件可复用的 `ui.viewport ^1`。图片尺寸、滚轮输入和原生变换由宿主公开提供；步长、缩放比例、自动适配和文件切换重置均由 Image 插件决定。需使用提供该能力的新版宿主；后续在已有能力范围内修改缩放策略只更新插件。

预览直接使用编辑器内的未保存内容，支持输入、粘贴、撤销及磁盘内容重新加载。源码输入暂时不完整时，右侧显示解析说明；修正后自动恢复预览。

SVG 保持原有颜色和透明度，绘制在白色／灰色棋盘之上。棋盘只覆盖 SVG 底座，周围保持面板背景。

SVG 提供以下缩放按钮。PNG、JPEG、GIF、WebP 同样支持滚轮缩放，并保持图片位于预览窗口中央；默认仍使用原尺寸／超区自动缩小，手动比例在窗口调整尺寸时保持不变，切换图片时重置。

- 在 SVG 预览图片区域滚动鼠标滚轮即可缩放；滚轮与所有缩放按钮均保持图片中心固定在预览画布中央。
- SVG 初次显示的最长边至少为 240px，小 SVG 自动放大，大 SVG 保持原尺寸；超出预览内容区域时等比缩小至完整可见。自动适配会跟随区域尺寸、文件切换和 SVG 画布尺寸变化；手动缩放后不再应用此默认尺寸。
- 左上角四个 SVG 图标依次为：放大、缩小、以原始尺寸缩放（1:1）、以窗口尺寸缩放。
- 右上角显示当前缩放比例，字体、字号和字重跟随编辑器默认 UI 文字样式；窄面板内比例移至工具栏第二行，避免覆盖图标。
- 以原始尺寸缩放会居中并恢复原尺寸；以窗口尺寸缩放会完整适应当前预览区域，并跟随分割线变化。
- 普通 SVG 支持 1%–3200% 缩放；极大或极小的 SVG 会扩展范围以支持默认尺寸和窗口适应，最终受绘图尺寸上限约束。
- 已放大的图片裁剪在预览面板内；改变分割线宽度会更新预览视口。

0.6.0 使用 `protocol = 7`，协商 `ui.native >=1.1`、`ui.canvas >=1.1`、`ui.viewport ^1`、`editor.layout`、`ui.tools`、`editor.documents`、`editor.files`、`ui.file_images` 与 `storage.private >=1.1`。权限 `editor.read` 用于当前 SVG 内存内容与文件上下文；`workspace.read` 用于受控图片资源；`storage` 用于私有显示意图。更新时新增权限须确认。每帧回传目标身份与 revision，旧版本不能覆盖新内容；同一路径重开使用新身份。图标由本包提供，普通 Canvas 承载矢量与指针事件。插件不申请进程或网络权限。

支持静态 SVG 的路径、颜色、渐变、透明度及嵌入资源；主程序负责原生文字和图像渲染。SVG 外部图片路径不会被读取。单个 SVG 源码上限为 1 MiB。其他格式通过独立文件版本与受控原生图片资源读取，要求 `editor.files`、`ui.file_images` 和 `workspace.read`；编码字节上限 8 MiB，解码尺寸／内存遵守宿主有限预算，超限、损坏和未授权均显示原因。

四个图标源文件位于 `icons/zoom-in.svg`、`icons/zoom-out.svg`、`icons/actual-size.svg`、`icons/fit-window.svg`，同时打包到 ZIP 中。图标颜色随预览面板主题变化。

```powershell
# 主程序提供协议与 WIT 接口缓存。
cargo build -p editor-app
./target/debug/editor-app.exe --plugin-cargo plugins/svg/Cargo.toml test --lib
./target/debug/editor-app.exe --plugin-package plugins/svg --output dist/plugins
# 通过真实 WASM 组件验证文档同步、缩放、解析恢复和权限。
cargo run -p plugin-runtime --example svg_preview_smoke -- dist/plugins/svg-0.6.0.zip
# 检查真实组件的原生颜色、透明洞和半透明像素，并输出预览图。
cargo run -p editor-app --example svg_preview_render -- dist/plugins/svg-0.6.0.zip plugins/svg/examples/gear.svg target/svg-render.png
# 使用隔离数据启动原生窗口，验证插件加载及正常关闭。
# 安装 ZIP 后在编辑器中检查预览、输入和资源撤销。
```
