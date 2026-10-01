# 议题跟踪器：GitHub

用户于 2026-10-02 确认采用 `T-miracle/Editor` 的 GitHub Issues，并批准插件平台 API 的 20 张工单拆分。

仓库：https://github.com/T-miracle/Editor

## 发布与读取

- 方案与实施工单分别创建 GitHub issue，长正文保留完整 Markdown。
- 优先使用可用 GitHub 连接器；连接器缺少原生关系操作时，可使用已认证的 GitHub REST API，或已安装的 gh CLI。
- 凭据仅用于既定仓库认证，不写入文件、日志或议题正文。
- 发布前按稳定标识和本地发布记录核对已有议题，避免中断重试产生重复议题。
- 未获明确请求不提交代码、关闭议题、指派执行人或启动实现。

## 阻塞关系

- 按依赖顺序发布工单，正文的 Blocked by 使用真实 issue 引用。
- 使用 GitHub 原生 issue dependencies；添加阻塞项时传递其数据库 ID，不使用 issue 编号替代。
- 仅当原生关系确实不可用时才使用正文作为回退，并明确记录限制。
- 设计规格是引用来源，不自动成为必须关闭的执行阻塞项；不关闭或改写父议题来推进工单。
- ready-for-agent 表示规格可执行；只有全部阻塞项完成的工单才处于实施前沿。

## 当前插件平台方案

- [设计基线 #1](https://github.com/T-miracle/Editor/issues/1)
- 实施工单 #2–#21，工单序号 01–20 与 GitHub issue 编号不同。
- [本地目录与依赖图](../specs/plugin-api-tickets/README.md)
- 发布记录与真实 ID 映射保存在本地工单目录的 publication.json。

## PR 作为请求入口

将 PR 作为请求入口：no。
