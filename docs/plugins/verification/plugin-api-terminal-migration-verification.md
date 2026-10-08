# 工单 17：终端公开能力迁移

状态：已完成（2026-10-03 状态汇总）。本记录保留该阶段实际执行的结果与限制；后续迁移、旧协议删除及最终交付以[完整契约验收](plugin-api-contract-verification.md)为准。20 张工单均已提交、推送并关闭；早期正文中的“尚未推送”“后续工单”是当时的历史状态。

对应 [GitHub #18](https://github.com/T-miracle/Editor/issues/18)。审查固定点为 `2018e97b18f70fc6b17be6e4ee31a016ca14d3f7`，包含工作区修改与新增文件。

## 交付行为

终端升级到 0.6.0、协议 7，独立通过宿主公开 SDK 编译为 WASM。进程、编辑器保存、剪贴板、私有设置文件和面板显隐均采用类型化能力；宿主不增加终端 ID、Shell 或 VT 分支。普通 Row 组合 SideTabs 与 Canvas，原生菜单、字符测量、画布字体和滚动范围均为公开 UI 能力。

`process` 1.2 增加显式执行的可选绝对 cwd；`ui.clipboard` 和 `storage.editor` 分别提供受权限约束的剪贴板与打开私有文件操作。异步粘贴绑定原进程句柄，切换会话不会把文本写入错误 Shell。运行项目只在带文档版本的保存成功后启动。输入绘制保持稳定交互版本，目标切换或模态改变才使旧事件失效。

按用户先前明确要求，内核改用直接 Git 依赖的上游 `term-wm-vt100`，固定修订 `5ffcf205c709b2378dc91cfa4637ae0f819d6c74`；移除全部本地 Alacritty vendor 文件，许可随 ZIP 分发。宿主操作系统决定省略的默认 Shell，WASM 编译目标不会令 Windows 回退到 `/bin/sh`。

逻辑快照 schema 2 仍读取 schema 1，保留样式、软换行、会话和历史；已退出会话只恢复历史，其他会话启动新 Shell，绝不重放旧命令。最小化/隐藏释放布局与输入所有权，保留会话运行；禁用和卸载进一步回收进程。这延续现有产品行为。迁移验证配置副本，失败恢复旧版本及数据，损坏文件不会被默认设置覆盖。

## 验证

| 检查 | 结果 |
| --- | --- |
| 工作区及终端 `cargo fmt --check`、`cargo check --workspace` | 通过 |
| `cargo test --workspace --exclude editor-app` | 55 项通过，46 项外部组件测试默认忽略 |
| 编辑器串行完整测试 | 173 项通过，22 项默认忽略 |
| 终端独立原生单元测试 | 43 项通过，包括 VT、重排、历史、快照、输入、配置与主题 |
| 宿主及真实终端 WASM/ZIP 构建 | 通过，包含 README 与上游许可 |
| 实际终端包 + Windows ConPTY | 空格/中文命令输出、异步粘贴归属、显式 cwd、保存失败阻止运行、更新失败回滚、配置迁移、退出会话恢复及卸载均通过 |
| 独立 SDK 夹具 + 原生 GPUI | 剪贴板读写、打开私有文件使用真实编辑器请求执行，结果通过 |
| 原生 SideTabs + Canvas | 布局、选择、拖动宽度及按实际字符行高换算滚轮通过 |
| 原生 Popup | 重复打开/Escape、遮罩及画布尺寸不变通过 |

同帧 Key/Text 初次暴露 StaleRevision，修复后真实 WASM 回归通过；滚轮初次暴露单行步长小于字符行高，修复后原生输入回归通过。弹出菜单挤压画布和第二次 Escape 失效均有失败再成功记录。热更新测试另外覆盖仅含 history/default_profile 的设置文件，省略 profiles 时仍能在 Windows 创建正确 Shell。

测试使用隔离目录，不修改用户真实安装记录。日志在忽略目录 `target/plugin-api-publication/terminal-*.log`，包括 `terminal-hot-migration-final.log`、`terminal-unit-final.log`、`terminal-app-final-tests.log`、`terminal-desktop-capabilities.log`、`terminal-wheel-green.log`。Windows 现有链接及死代码警告仍存在；未宣称其他系统的原生运行验收。

## Standards

独立审查发现菜单重复关闭状态和布局两项问题，均已修复并通过原生 GPUI 回归。异步请求绑定原进程、配置错误不覆盖用户数据的补充复核通过。最终硬性违规与判断性坏味道均为 0 项。

## Spec

独立审查发现同帧输入版本、滚轮行高和旧 README 问题，均已闭合；最终静态复核无遗留问题。通用旧协议适配器的最终删除仍由 #21 执行。
