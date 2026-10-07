---
title: 语义视口
description: 公开能力、权限与生命周期。
section: sdk
order: 12
alternate: /en/sdk/viewport/
---

# 语义编辑器视口 — editor.viewport 1.0

此附加能力将原生源码编辑器与带源码映射的预览绑定，不改变文本、选区、焦点、IME、折叠或 Undo，不创建第二份文档或滚动句柄。

## 声明与授权

Document.editor_viewport 指定一个活动根 Scroll 的 ID；Document.source 精确回显获准 Preview 输入。自有工作区编辑区面板须协商 editor.viewport、ui.richtext 并拥有 editor.read；普通面板默认不绑定。节点使用有界 SourceRange UTF-8 映射，定位前由宿主验证实际源码边界。

插件通过 `ui.tools` 和 `storage.private` 提供同步控件、默认值及工作区／文件类型偏好，仅在当前布局和意图要求同步时发布 `editor_viewport`。宿主不提供链控件或固定显示模式。撤回绑定、隐藏一侧、切换来源、打开模态窗口、实例退役或移除贡献会取消待完成测量和定位；保留的只读绘制不能执行旧定位。

## 通知与请求

Notification::SourceViewport(SourceViewport) 包含当前文档／UI revision、视口顶部视觉光标 offset、该视觉行超出视口的比例、可选 origin 和 layout 标记。Action::Viewport(PreviewViewport) 由声明的 Scroll 发出，指定测得源码块、精确源码范围及超出视口的比例。通知合并为每次完成原生布局最多一次；比例须为有限的 [0,1]，源码偏移／范围不超过 1 MiB，块 ID 最多 128 UTF-8 字节。

EditorOperation::LocateViewport { document, panel, ui_revision, target, origin } 使用 ViewportTarget::Source { offset, line_fraction } 或 Preview { node, fraction }。非零 origin 标识程序定位；采用普通作用域编辑器请求的取消、截止时间和队列额度。Unit 表示在当前授权下排队原生定位；后续每次布局重新核对源码／UI 身份、分屏模式与偏好。绘制前几何失效时丢弃，不应用到新场景。

程序移动回显 origin: Some(...)，人工输入无 origin；访客不能将程序回执反向定位。layout 区分图片／宽度／换行重排和人工垂直滚动，访客可保留最后人工驱动方并刷新跟随侧。不能仅为确认视口事件重发 UI 文档；快速输入合并时最多保留一个待完成请求和一个最新位置。

源码定位使用公开 Base 原生布局和有界搜索（最多 48 次布局），不假定缓冲区行等于换行后视觉行。几何缺失或折叠可失败，不隐式展开。预览按当前测得块高度和最近 Base 滚动归属定位，图片或表格不采用全文百分比映射；文末定位夹紧至真实可用范围。

## 失败与生命周期

未协商返回 CapabilityUnavailable；作用域／授权／归属错误返回 PermissionDenied；origin、比例、目标或 UTF-8 边界非法返回 InvalidRequest。关闭、切换、编辑来源及替换 UI 场景返回 StaleRevision；隐藏或关闭分屏同步返回 InvalidState。原生指针手势期间定位返回 Cancelled，人工归属保留至释放，包括移出面板。排队成功与取消都不声称覆盖后续人工滚动。

分发 SDK 含请求／通知类型和本文；独立访客与内置预览使用同一公开契约。
