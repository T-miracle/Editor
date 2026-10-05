# 工单 04：真实插件与历史显示偏好迁移验收

日期：2026-10-06。议题：[#65](https://github.com/T-miracle/Editor/issues/65)。
分支：`codex/plugin-ui-decoupling`；固定审查起点：`4d4a7b6`。
状态：阶段验收通过，等待固定提交审查，尚未关闭工单。

## 交付范围

Markdown 0.12.0 与 Image 0.5.0 使用 `editor.layout` 和 `ui.tools` 选择中心布局及底栏功能。
Markdown 的同步和源码格式工具栏显隐、两种 Markdown 后缀共享的显示设置均在访客内处理。
Image 保留稳定插件 ID `svg`；仅有文本能力的 SVG 绑定源码布局偏好，PNG 等图片没有编辑器或源码模式按钮。
Terminal 0.8.0 使用窗口目标工具提供新建和菜单；窗口显隐仍由左组的通用宿主入口控制。

文件与工具图标移入包资源。终端 ANSI 和域内默认值来自包内 `theme.json`，Markdown 提供内容默认颜色。
`ui.content_colors 1.0` 仅传递有界 RGB 角色，用户主题覆盖优先；原生控件行为与外观仍由宿主通用 UI 层实现。
`ui.native 1.1` 在 Row／Column 中提供 Base 可调整分栏，不恢复固定三模式宿主分支。

`PreferenceBinding` 统一 guest 的有界 CAS、watch 与关闭传输，具体 schema 和默认模式留在插件。
宿主仅把旧 session 三组字段作为不透明记录导入已授权私有存储；已有新记录优先，损坏数据保留。
只有成功写入且来源仍一致才移除对应 session 导入记录，其他设置不变。
SDK 示例 0.15.9 重新绑定通用原生编辑引用，没有文本时不发布无归属的编辑器。

## 已取得的行为证据

- 协议测试 36 项通过，涵盖 RGB 角色预算与可调整分栏的序列化、合法容器限制。
- Markdown guest 66 项、Image guest 7 项通过，包含有限旧 schema、未来／损坏记录拒绝和超量解析回退。
- Terminal guest 修复用户字体优先级后 43 项通过，保留 ANSI、输入、历史、侧栏及主题覆盖。
- 真实 Markdown 源码／分栏／预览切换 3 项通过；完整中文格式、空选区模板与所有命令／取消回归 3 项通过。
- 真实包历史偏好导入、已有插件偏好优先与重启恢复通过；SVG 源码偏好不影响 PNG 通过。
- 500,008 字节 Markdown 在后台预览暂停时连续输入 `a`、`b`、`c`、中文，源码与旧只读树保持可见，焦点保留；本次测量约 400 毫秒，随后精确更新当前 revision。
- 嵌套任务列表在默认线程栈下通过真实鼠标和键盘回归。递归节点路由与非递归装饰构建分离，避免每层持有大型 GPUI builder 栈帧。
- 刚建立中心提供者但尚未返回首个树时继续显示原生文本；保留的旧树只有绘制权限，所有访客事件、工具与视口定位仍要求精确当前版本。
- 跨文件标题链接先核对当前场景并准备原生面板，再由下一次布局测量执行定位。原用例稳定复现的“Preview has not been drawn”已修复；两种 Opened／Preview 到达顺序和取消旧同步请求的原生回归均通过。
- 运行时实际包 `composable_ui` 10 项、`plugin_tools` 5 项、`terminal_migration` 1 项通过；包含新配色／分栏协商、有限偏好导入、授权撤销及真实 ConPTY 清理。`file_views` 1 项和 Image 旧内容拒绝通过，示例缺失夹具按脚本补建后其恢复回归也通过。
- 最终阶段基础检查 `cargo fmt --check`、`cargo test --workspace --exclude editor-app`、`cargo check --workspace` 通过。构建宿主后，独立 SDK 导出、损坏缓存修复和仓库外组件构建通过；最新 Markdown 66 项与 Image 7 项 guest 单元测试通过。

首次批量失败的 7 项全部复核通过；原生增量组合 18 项通过，包含首次安装／授权 9 项、导航同步 2 项、代码提供者、网络图片、迁移 4 项及超量预览恢复。另一次单项跨文件定位通过。完整应用测试最终串行 248 项通过、106 项 ignored；基础检查非 UI workspace 为 125 项通过、121 项 ignored。未执行的 ignored 不计为通过，真实包结果按下列显式命令单独记录。

## 本阶段命令与日志

以下路径均相对本次隔离工作区，`target/` 日志不作为发布产物提交。

```powershell
# 插件只能使用宿主提供的 SDK；直接 cargo test 会尝试从 registry 查找未发布的协议包。
./target/debug/editor-app.exe --plugin-cargo plugins/markdown/Cargo.toml test
./target/debug/editor-app.exe --plugin-cargo plugins/svg/Cargo.toml test
./target/debug/editor-app.exe --plugin-cargo plugins/terminal/Cargo.toml test
./scripts/build-plugins.ps1 -Packages markdown,svg,terminal,example -HostExe ./target/debug/editor-app.exe
cargo test -p plugin-runtime --test plugin_tools --test composable_ui --test terminal_migration -- --ignored --test-threads=1
cargo test -p plugin-runtime --test file_views --test ui_migrations -- --ignored --test-threads=1
cargo test -p editor-app --no-run
./target/debug/deps/editor_app-9a31643de7b6e6bd.exe extensions::markdown_tests --ignored --test-threads=2
```

首次 Markdown 批量 69 项中 62 项通过。7 项失败分别为跨文件定位 3 项、旧资源／版本夹具 2 项、全局语言注册并发干扰 1 项及持有网络图片的计时回归 1 项；按根因修复或隔离后只重跑受影响组合。代码提供者与持有网络图片的单独串行复核通过，前者 59.07 秒、后者 23.23 秒；没有放宽原生输入的 2 秒上限。应用的全局语言注册测试最终串行运行，不把并发污染误改成产品行为。

`migration-runtime-real.log` 记录真实协议／工具／终端结果，`migration-delivery-*.log` 记录基础检查、SDK 和 guest 单测；`migration-navigation-fixed.log` 记录定位修复，临时观测已从源码删除。原生增量与完整串行应用结果记录在 `migration-native-delta-*.log`、`migration-native-serial.log`。

## 实际限制

Markdown 仍按只读源快照派生预览，本批未复制原工作区未提交的增量解析性能改动。
超出原生节点、深度、事件或单叶文本预算时显示明确限制，编辑器继续可用；不承诺任意大文档完整预览。
原生交互证据来自 GPUI 测试窗口真实事件和绘制，不把编译或内部状态断言当作手工 UI 验收。

## Standards

等待固定提交双轴审查。

## Spec

等待固定提交双轴审查。旧公开呈现契约的彻底收缩与全 U01–U20 组合验收归 #66，不提前记为完成。
