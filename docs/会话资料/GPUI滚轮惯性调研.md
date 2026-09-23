# GPUI 鼠标滚轮惯性与顺滑滚动调研

调研日期：2026-09-23

## 结论

可以在应用层实现顺滑、带惯性的滚动，但当前项目所锁定的 GPUI 并没有一个能对普通鼠标滚轮全局开启惯性的简单配置项。GPUI 的内置动量机制主要用于触摸/拖动手势；滚轮事件则由输入设备和操作系统提供增量，滚动容器按增量更新位置。

本项目 `Cargo.lock` 锁定 `gpui-pre 0.3.6`，该 crate 对应 Zed 源码快照 `bcf6582ce3500df93a8a39366640173e6786cea6`。[版本信息](https://docs.rs/crate/gpui-pre/0.3.6)

## 为什么普通滚轮会一顿一顿

- GPUI 的滚轮事件包含滚动增量（像素或行）和触摸阶段等信息；它不是一个自动带动画的“目标滚动位置”请求。[GPUI 输入事件源码](https://github.com/zed-industries/zed/blob/bcf6582ce3500df93a8a39366640173e6786cea6/crates/gpui/src/interactive.rs)
- 通用 `Div` 滚动监听器会将当前事件的增量换算成位移并应用到滚动偏移，因此普通离散滚轮输入会呈现逐格移动。[GPUI Div 滚动实现](https://github.com/zed-industries/zed/blob/bcf6582ce3500df93a8a39366640173e6786cea6/crates/gpui/src/elements/div.rs)
- GPUI 确实实现了 `GestureTuning`、`ScrollPhysics` 和动量 tick，但这套逻辑属于触摸拖动识别/释放后的 fling，不是供普通鼠标滚轮调用的通用开关。[GPUI 手势源码](https://github.com/zed-industries/zed/blob/bcf6582ce3500df93a8a39366640173e6786cea6/crates/gpui/src/gestures.rs)
- GPUI Kit 的 `Scrollable` 文档介绍滚动容器、滚动条外观与显示方式，没有列出滚轮惯性配置。[GPUI Kit Scrollable 文档](https://gpui-kit.com/docs/components/scrollable/)

因此，若 Windows 鼠标按滚轮刻度发送离散“行”增量，GPUI 通常就会按每次增量移动；精密触控板或其他发送连续像素增量的设备则能获得更连续的输入，惯性还可能由系统/设备提供。

## 对本编辑器的影响

项目中 tab 栏有自己的 `.on_scroll_wheel` 处理器，并直接调用 `tabs_scroll.set_offset(...)`。因此即使其他区域使用 GPUI 的通用 overflow 滚动，要让 tab 栏也有一致的缓动/惯性，仍需要在这个自定义处理器中单独处理动画。

## 可行方案

1. **应用层平滑动画（推荐）**：收到滚轮增量时不立刻跳到终点，而是更新目标偏移；通过 GPUI 帧/定时器逐步逼近目标，或按速度衰减模拟惯性。需要为编辑器正文、目录树、输出面板以及自定义 tab 栏分别确认事件和滚动状态的接入点，并限制偏移在内容边界内。
2. **依赖系统/设备输入**：使用触控板或能输出精细像素滚动的设备；实际效果受 Windows、驱动和设备设置影响，不保证普通滚轮也变成惯性滚动。
3. **系统滚轮行数**：降低每格滚动行数可以减小跳动幅度，但仍然是离散事件，不等价于惯性动画。

## 判断

结论是“能做，但不是 GPUI 中打开一个惯性选项即可”。如果后续决定实现，应用层目标位置 + 帧间缓动通常更适合编辑器；要注意滚轮输入新到达时如何合并目标、如何停在边界，以及用户偏好减少动态效果时如何降级。
