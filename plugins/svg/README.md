# SVG 插件

安装 `svg.zip` 后，打开 `.svg` 文件会将编辑区显示为左侧源码、右侧预览；中间的分割线可以拖动。切换到其他文件时，恢复普通编辑区。文件扩展名不区分大小写。

预览直接使用编辑器内的未保存内容，支持输入、粘贴、撤销及磁盘内容重新加载。源码输入暂时不完整时，右侧显示解析说明；修正后自动恢复预览。

SVG 保持原有颜色和透明度，绘制在白色／灰色棋盘之上。棋盘只覆盖 SVG 底座，周围保持面板背景。

- 在预览图片区域滚动鼠标滚轮即可缩放；滚轮与所有缩放按钮均保持图片中心固定在预览画布中央。
- 初次打开时，图片最长边按 240px 展示，并保持宽高比；拖动分割线不会改变默认尺寸。
- 左上角四个 SVG 图标依次为：放大、缩小、以原始尺寸缩放（1:1）、以窗口尺寸缩放。
- 右上角显示当前缩放比例，字体、字号和字重跟随编辑器默认 UI 文字样式；窄面板内比例移至工具栏第二行，避免覆盖图标。
- 以原始尺寸缩放会居中并恢复原尺寸；以窗口尺寸缩放会完整适应当前预览区域，并跟随分割线变化。
- 普通图片支持 1%–3200% 缩放；极大或极小的 SVG 会扩展范围以支持默认尺寸和窗口适应，最终受绘图尺寸上限约束。
- 已放大的图片裁剪在预览面板内；改变分割线宽度会更新预览视口。

0.2.1 使用 `protocol = 7`，协商 `ui.native`、`ui.canvas >=1.1` 与 `editor.documents`。唯一权限 `editor.read` 用于接收当前 SVG 内存文档，包括未保存内容。宿主按面板声明管理 Preview 订阅；每帧回传来源文档 ID、路径与 revision，旧版本不会覆盖新内容，同一路径重新打开也使用新的文档身份。普通 Canvas 承载矢量、主题字体和原生指针事件，宿主无需识别 SVG 插件 ID。插件不申请进程、网络、工作区文件或私有文件读写权限。

支持静态 SVG 的路径、颜色、渐变、透明度及嵌入资源；主程序负责原生文字和图像渲染。外部图片路径不会被读取。单个 SVG 源码上限为 1 MiB，超限时显示说明。

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
