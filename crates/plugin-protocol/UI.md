# 插件原生界面协议 v1

## 可组合布局（清单 protocol 7）

`ui.collections ^1` 增加 `Kind::SideTabs` 与 `Document.menu`。SideTabs 是普通布局节点，可在行列树任意位置组合；节点 ID 与集合 ID 相同，条目动作仍按稳定 ID 返回。插件通过节点宽度响应 `Resize(width)`，相邻画布会得到独立的实际尺寸测量。PopupMenu 锚点相对文档，覆盖正文、不占布局空间；显示时只接受该菜单的选择或关闭，Dialog 优先。

`ui.canvas >=1.1` 增加 `Canvas.font` 和 `Canvas.scroll: Option<ScrollRange>`。字体覆盖画布继承值并参与网格测量；范围的 `content/offset` 使用逻辑像素，拖动原生滚动条产生 `CanvasEvent::Scroll { offset }`，宿主不保存第二份终端内容。字符网格的行滚轮以 `GridMetrics.cell_height` 换算，普通画布保持像素事件。

文档 revision 标识交互对象及语义，不是绘制帧计数。输入目标、会话/进程身份或模态变化应递增；普通输出、绘制、按键反馈保持版本，避免同一帧的 Key/Text 或在途输入被错误判为过期。插件异步调用的后续操作应绑定最初目标的稳定身份，目标撤销后丢弃，不能跟随当前焦点。

新插件协商 `ui.native ^1`，通过 `api::Output.views` 返回 `ui::Document`。`Column`、`Row`、`Scroll`、`Tabs` 可在任意位置嵌套标准组件和 `Kind::Canvas`，没有终端专属槽位。画布另需 `ui.canvas ^1`，字符网格仅在协商 `ui.grid ^1` 并设置 `Canvas.grid=true` 时出现。UI 能力不授予文件、文档或进程权限。

`Canvas.paint` 使用通用 `Paint::Fill/Text/Svg`；SVG 由后台受限渲染器绘制，禁止环境中的文件与网络读取。每份文档最多 2048 个节点、32000 个绘制操作、16 个 SVG，序列化后最多 2 MiB。无效布局在发布之前拒绝。原生文字采用宿主字体；插件通过 `Notification::Theme` 更新自己的绘图颜色、字号，尺寸变化通过画布节点的 `CanvasEvent::Resize` 热更新。

每个 `UiEvent` 包含当前文档 revision 和节点 ID。宿主先验证活动节点、模态范围与事件类型；过期版本返回 `StaleRevision`，不存在或禁用节点返回 `InvalidHandle`，事件类型不匹配返回 `InvalidRequest`。这些被拒绝的宿主回调不会使插件崩溃。稳定节点 ID 保留输入框焦点与组合输入。交互画布须显式设置 `focusable=true`，IME 的未提交文本保留在宿主，提交后通过 `CanvasEvent::Text` 发给该画布；相邻输入框独立处理自己的输入。鼠标按钮编号为左 0、中 1、右 2，坐标相对画布节点。

编辑区预览声明 `position:"editor"` 和 `file_extensions`，使用工作区实例，申请 `editor.read` 并协商 `editor.documents ^1`。宿主发送 `Notification::Preview { document, text }`，其中 `DocumentVersion` 指向内存文档，`text` 包括未保存修改。插件必须在 `ui::Document.source` 原样返回版本；过期预览不会替换新文档。`document:None` 与空文本表示撤销当前预览。普通面板 `source` 留空。隐藏或卸载后销毁原生事件目标并移除其布局空间。

独立 SDK 示例见 `plugins/capability-example/src/composition.rs` 与同目录 `composed-ui.json`；将示例的 `label` 配置为 `composable-ui` 可展示组合界面，`ui-layout` 命令参数 `form/canvas/combined` 切换三种布局。以下 protocol 2–6 文档仅适用于迁移期间的旧协议。

## 编辑区预览与彩色 SVG 绘制（清单 protocol 6）

面板使用 `position: "editor"` 与非空 `file_extensions`（扩展名不含点，大小写不敏感）。需要在清单中申请并获得 `editor.commands`。宿主选择当前文件匹配的可见预览，将原生编辑器和插件面板放入可拖动的左右分栏；预览不加入外部停靠树。多个插件匹配时按插件／面板 ID 排序选择首个。

`Event::Surface { panel, event: Event::Document { path, text } }` 提供当前内存内容，包括未保存修改。`path: None` 和空 `text` 表示离开这个文档。热更新后宿主会重新发送当前文档。插件不需要读取文件或替换编辑控件。

画布可发送 `Paint::Svg { rect, clip, source }`：`rect` 是图像的目标尺寸和位置，`clip` 是可见视口。宿主后台按可见交集渲染，保持 SVG 颜色与 alpha；绘图按列表顺序叠加，因此棋盘应放在 SVG 操作之前。SVG 源码每项最多 1 MiB，一份场景最多 16 项、合计最多 2 MiB。每张可见栅格最多 2048×2048 像素，缩放时重新绘制可见区域；外部文件／URL 图像不解析，嵌入资源可用。

文件预览事件与 SVG 绘图需要 protocol 6；旧宿主拒绝新包，避免安装后出现错误停靠或空图像。WIT 世界保持不变。完整独立插件见 `plugins/svg`。

## 画布与原生菜单、侧边 Tab 栏组合（清单 protocol 5）

画布插件使用 `Scene.controls: Option<ui::CanvasControls>`。字符网格留在 `Scene.paint`，侧边栏和菜单交给宿主原生模块；不与 `Scene.ui` 或旧 `widgets` 混用。`CanvasControls.revision` 随 `UiEvent` 返回。宿主仍接受旧插件发出的 `chrome` 字段，新插件统一发出 `controls`。

- `SideTabs`：稳定条目 ID、完整名称、选中项、可关闭/禁用状态、宽度范围、可选重命名目标。`position: SideTabsPosition` 可选 `Left` / `Right`，JSON 为 `"left"` / `"right"`，省略时沿用右侧停靠。滚动、省略显示、双击编辑、Enter/失焦提交、Escape 取消、关闭、中键关闭、拖动排序及宽度拖动由宿主管理。调整宽度的手柄和选中项的内侧边线始终朝向画布，左停靠时位于右侧。
- `PopupMenu`：面板内锚点和菜单项；支持禁用、分隔线、鼠标悬停、方向键、Home/End、Enter、Escape、外部点击关闭及原生滚动。卡片和条目样式与资源管理器右键菜单共用。
- 事件：侧边栏返回 `Select(id)`、`Close(id)`、`Rename { id, value }`、`Move { from, to }`（移动到目标原索引）、`Resize(width)`、`Context { id, x, y }`（坐标相对侧边栏）；菜单返回 `Select(command_id)` 或 `Dismiss`。插件按 ID 修改业务状态并返回新场景，无需做控件坐标命中。
- 画布仍收到完整面板的 `Resize`；插件按 `sidebar.width` 为声明的一侧预留空间，左侧布局需要同步平移绘制和鼠标命中坐标。菜单锚点以面板左上角为原点，由宿主限制到窗口可见范围。
- 配色使用 `plugins[ID].ui.tab_bar`、`ui.tab.active/inactive/close/rename`、`ui.menu`，字体使用 `typography.tab/menu`。标签栏背景默认 `#F7F8FA`，选中项保留完整边框，朝向画布的 1px 边线默认 `#3574F0`，可用 `tab.active.inner_border` 覆盖。标签和关闭按钮不使用悬停变色。其余未覆盖项继承当前系统/安装主题，并复用 `components.explorer_menu` 的菜单样式。
- 每组最多 512 项，单项标签最多 4 KiB、总标签最多 64 KiB，条目 ID 唯一且不含控制字符。侧边栏宽度为有限的 0–1200 数值，并满足最小/当前/最大宽度顺序。

完整接入示例见 `plugins/terminal/src/controls.rs`；旧的清单 protocol 1/2/3/4 继续可用，其中 protocol 3 插件的 `chrome` 字段由新宿主兼容读取。支持左右摆放的插件声明 protocol 5，使旧宿主明确拒绝无法正确布局的安装包；旧插件省略 `position` 时仍使用右侧布局。WIT 世界不变。

需要插件清单 `"protocol": 2`。旧宿主会拒绝安装该插件，避免把新界面静默显示成空白；新版宿主继续支持 protocol 1 的画布插件。WIT 世界不变，原生界面通过 `Reply.scene` / `Reply.scenes` 的 `Scene.ui` 传递。

编译接口由已打包的编辑器通过 `--plugin-cargo <Cargo.toml> build --target wasm32-wasip2 --release` 自动准备到用户缓存。插件使用编辑器提供的 Rust 类型和 WIT 绑定，不需要项目内的 SDK 目录，也不依赖主程序源码或 GPUI。主程序验证界面树后，在原生 UI 线程使用 gpui-base 的控件行为绘制。

## 最小示例

```rust
use plugin_protocol::{Scene, Event};
use plugin_protocol::ui::{Action, Document, Node};

fn view(count: u64) -> Scene {
    Scene {
        panel: "counter".into(), // 必须在 manifest.panels 声明
        ui: Some(Document::new(
            Node::column("root", vec![
                Node::text("count", format!("次数：{count}")),
                Node::button("increment", "增加一次"),
            ]).padding(12.).gap(8.)
        ).revision(count)),
        ..Default::default()
    }
}

fn event(message: Event, count: &mut u64) {
    match message {
        Event::Surface { event: inner, .. } => event(*inner, count),
        Event::Ui(e) if e.node == "increment" && e.action == Action::Click => *count += 1,
        _ => {}
    }
}
```

完整可运行示例位于独立插件项目 `plugins/example`（`src/views.rs` 与 `src/lib.rs`）。

线上传输仍是 JSON。`Scene.ui` 的最小按钮文档如下；省略的布局、禁用和角色字段使用默认值：

```json
{
  "version": 1,
  "revision": 3,
  "root": {
    "id": "save",
    "kind": { "type": "button", "label": "保存" }
  }
}
```

点击后 `UiEvent` 的 JSON 为 `{"revision":3,"node":"save","action":{"type":"click"}}`；输入变化的 action 为 `{"type":"change","value":"新内容"}`。`revision` 表示产生操作的界面版本，与协议 `version` 分开。

## 常用界面元素

| Kind / 构造方法 | 用途 | 事件 |
| --- | --- | --- |
| `Column` / `Node::column` | 纵向自动布局 | — |
| `Row` / `Node::row` | 横向自动布局 | — |
| `Scroll` / `Node::scroll` | 带原生滚动条的纵向滚动区，设置 height 约束视口 | — |
| `Text` / `Node::text` | 普通文字，按容器换行 | — |
| `Button` / `Node::button` | 原生按钮，支持键盘激活 | `Click` |
| `Input` / `Node::input` | 单行原生编辑，支持中文 IME、选区、撤销与复制粘贴 | `Change(String)`、`Submit(String)` |
| `Checkbox` / `Node::checkbox` | 复选框 | `Toggle(bool)` |
| `Choice` | 带稳定选项 ID 的单选组；选项可禁用 | `Select(option_id)` |
| `Tabs` | 标签条与当前页内容 | `Select(tab_id)` |
| `List` | 只读文字列表 | — |
| `Table` | 只读文字表格 | — |
| `Separator` | 分隔线 | — |
| `Progress` | 0–100 的确定进度条，带无障碍标签 | — |
| `Spacer` | 占位空间，可设置宽高或 grow | — |
| `Document.dialog` | 复用主程序原生控件的模态浮层 | `Dismiss` |

`Node::new(id, Kind::...)` 可构造全部类型。`gap/padding/width/height/grow/role/disabled` 构造方法对应公开的 `layout/role/disabled` 字段。布局尺寸为逻辑像素。`disabled` 作用于整个子树；隐藏标签页和模态弹窗背后的节点不会收到操作事件。

## 身份、状态与事件

- 节点 ID 在一个面板的文档（包括所有标签页和弹窗）内唯一且稳定，允许 ASCII 字母、数字、点、下划线、连字符。主程序按 ID 保留输入状态和滚动位置；不要每帧生成随机 ID。
- 每次插件响应可以提交一份完整文档，主程序按 ID 协调原生状态。`revision` 由插件维护并随事件返回，插件可据此辨认旧界面产生的操作；事件不是 GPUI 回调或内存指针。
- 事件外层是 `Event::Surface { panel, event: Event::Ui(...) }`。`UiEvent.node` 为节点 ID，`action` 为类型化事件。选项和标签返回稳定 ID，不返回数组下标。
- 输入框的 `value` 是初始值。`value_revision` 不变时，主程序保留正在编辑的草稿，避免较慢的插件回复覆盖新输入。插件响应 `Change` 更新业务状态即可；清空、加载或重置输入时，增加 `value_revision` 才会替换原生草稿。程序性替换不会触发 `Change`。
- 按钮与选择类控件由插件状态控制：收到事件后返回新的 `checked/selected`。插件卸载、替换或切换回画布时，原生输入与订阅随视图释放。

## 弹窗

```rust
use plugin_protocol::ui::{Dialog, Document, Node};
let document = Document::new(Node::button("open", "打开"))
    .dialog(Dialog::new("settings-dialog", "插件设置",
        Node::column("settings-body", vec![
            Node::text("hint", "弹窗内容也使用同一套节点"),
            Node::button("save", "保存"),
        ]).gap(8.)
    ));
```

点击关闭或按 Escape 返回 `Dismiss`（node 为弹窗 ID）；插件应将下一份文档的 `dialog` 设为 `None`。确认按钮的语义由插件定义。等待插件回复期间继续保持模态，重复关闭请求只发送一次。弹窗关闭后恢复之前的焦点。协议 v1 每个面板最多一个模态浮层，尚不提供任意系统顶层窗口或嵌套弹窗。

## 主题和字体

节点只声明样式角色，没有硬编码颜色或字体字段。省略 `role` 时，使用 `button/input/checkbox/choice/tabs/text/list/table/progress/separator/scroll/spacer/container`；弹窗外壳使用 `dialog`。

主题在 `themes[].plugins[插件ID]` 中配置，示例：

```json
{
  "ui": {
    "button": {
      "background": "#243044",
      "foreground": "#FFFFFF",
      "hover_background": "#33455F",
      "active_background": "#405777",
      "border": "#62738A"
    },
    "choice": { "accent": "#66B3FF", "accent_foreground": "#101820" }
  },
  "typography": {
    "button": { "family": "Segoe UI", "size_px": 14, "bold": true }
  }
}
```

各角色支持 `background/foreground/border/hover_background/active_background/accent/accent_foreground` 中适用于该控件的颜色。未声明项继承编辑器当前主题。字体角色继承主题 UI 字体。主程序切换主题时直接重绘已有树，同时继续发送 `Event::Theme` 给插件。

## 约束与兼容

- `Document.version` 当前必须为 1；构造器自动设置。
- 单文档至多 2048 个节点及展开后的选项、列表行和表格单元格，嵌套深度至多 24。单段文字至多 64 KiB，总文字至多 1 MiB。
- 宽高必须为有限的 0–10000 数值；间距和内边距为 0–256；弹窗宽度为 240–1200；表格为 1–32 列，每行列数一致。
- 重复 ID、无效选中值和未知协议会被拒绝。一次 Reply 的所有场景先验证，再一起发布，避免部分界面更新。
- `Scene.ui` 与 `paint/widgets/scroll/column_resize_regions` 互斥；终端等画布插件仍可用现有协议。当前尚不支持在界面树里嵌入画布、图片、富文本编辑器、虚拟表格或自定义 GPUI 元素。

验证协议可运行 `Document::validate()`；SDK 随附契约测试。主程序另有原生点击、输入、弹窗与实际 WASM 包回归测试。
