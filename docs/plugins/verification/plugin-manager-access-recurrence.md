# 默认构建目录访问拒绝复发验收

日期：2026-10-06，Windows。本轮处理用户再次报告的“插件管理器：拒绝访问。(os error 5)”。
状态：本机目录标签恢复、默认 Cargo release 启动及原生插件管理器验收通过；重新加入 Low 标签的外部来源尚未确定。

## 为什么这次仍会失败

用户启动的是原工作区 `target/release/editor-app.exe`，初始 PID 30716。仓库根目录、debug 和 release EXE 均读回可继承的 `Low Mandatory Level`。父目录 `C:/Projects/RustProjects` 没有该标签，按 Windows 规则为普通 Medium。

[上一轮 main 合并](plugin-ui-decoupling/06-local-main-merge.md)在独立工作区的正常输出目录验证了相同 main 源码，但保留了原目录标签问题；其启动成功不代表原目录的 `cargo run --release` 已恢复。本轮原生观察再次确认旧进程的插件管理器显示拒绝访问及空列表。

公开 `Manager::read_registry` 首先进入数据事务恢复，其锁文件以读写方式打开。Low 进程无法写入普通 Medium 用户插件目录，即使用户具有 FullControl 也会被拒绝。此规则及创建进程时采用用户／EXE 较低完整性级别的行为见 [Microsoft MIC 文档](https://learn.microsoft.com/en-us/windows/win32/secauthz/mandatory-integrity-control)。

## 能变红的真实反馈循环

一次性诊断保留在忽略目录 `target/manager-access-probe/`，源码带注释，只调用公开 `Manager::read_registry`，不启动 WASM。它显式接收实际用户插件目录，失败返回退出码 1。构建使用现有依赖锁文件及 offline 模式。

```powershell
# 在原仓库调用真实插件元数据恢复／读取入口。
& ./target/debug/manager-access-probe.exe (Join-Path $env:APPDATA 'MeEditor/runtime-plugins')
```

- 修复前原目录连续两次：`FAIL plugin manager: 拒绝访问。 (os error 5) elapsed_ms=0`。
- 同一字节 EXE 普通复制到用户临时目录，连续两次：`PASS registry entries=5 elapsed_ms=5`、`elapsed_ms=1`。
- 两个 EXE 的 SHA-256 都是 `EE5E2483026615B2F6A80DD35EDA9E0879BD08BDA2C74A4728FF37E37A6AD454`；只有位置及继承标签不同。
- 原目录标签恢复后，同一原目录命令变绿：`PASS registry entries=5 elapsed_ms=1`。

对照排除了插件数据目录普遍不可访问和事务锁占用；失败发生在文件读写打开阶段，正常目录进程使用相同数据路径即通过。没有因报错清空或重装用户插件目录。

## 本机修复范围与保护

先验证仓库真实绝对路径、根目录非 reparse point，并保存 7 个路径的 DACL、owner、group 和 release 文件 hash。仓库当前有 `website/node_modules` junction；先仅移开链接对象，标签传播结束后原样恢复，避免传播到链接外的目录。

```powershell
# 验证实际仓库路径并隔离 junction 后，恢复目录及普通构建输出的继承标签。
icacls 'C:/Projects/RustProjects/Editor' /setintegritylevel '(OI)(CI)M'
```

未使用 `/T`、`/reset` 或 `/grant`。7 个监测路径的 DACL、owner、group 修复前后完全相同，原 release 字节也相同。根目录、debug／release 目录、两个宿主 EXE 和诊断 EXE 均恢复 Medium；多次后续工具调用后仍为 Medium。

目录继承选项及标签修改接口见 [icacls 文档](https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/icacls)；既有子项的继承传播规则见 [SetNamedSecurityInfoW](https://learn.microsoft.com/en-us/windows/win32/api/aclapi/nf-aclapi-setnamedsecurityinfow)。目录标签是本机文件元数据，Git 合并不能恢复或传播这些标签。

## 结果

| 实际验证 | 结果 |
| --- | --- |
| 原目录真实 Manager 读取 | 标签恢复后通过，5 条安装记录 |
| 新 Cargo 输出继承 | 移开一次性诊断 EXE 后重新构建，新 EXE 自动为 Medium，真实 Manager 读取通过 |
| 默认目录状态保存测试 | `cargo test -p editor-app legacy_display_import_preserves_future_and_unrelated_session_data -- --test-threads=1`：1 passed，0 failed，0 ignored |
| 正常重启原程序 | 经原生关闭请求退出旧进程，未强制结束或丢弃文件；重新打开保留原有 7 个文件 Tab |
| 默认 `cargo run --release -- C:/Projects/RustProjects/Editor` | 构建通过，17.60 秒；直接启动原目录 release EXE，PID 25300 |
| 原生插件管理器 | 显示 HTML 0.2.1、Markdown 0.15.0、Rust 0.2.1、SVG 0.3.0、终端 0.7.1；拒绝访问红字消失 |
| 已安装数据 | 5 个插件的身份、版本、包 digest、启用选择和 grants 全部保留；公开读取按当前契约正规化旧 manifest 并补充 `retired_ui_contract` 标记，原始 registry 已备份 |

本轮未变更产品 Rust 代码或插件包，未执行 Git 提交或推送。格式／workspace 完整代码验证复用上轮记录，追加了本次失败路径、默认输出继承、实际 release 构建及原生验收。当前 workspace 的完整 `git diff --check` 另发现用户正在修改的根 README 第 8 行尾随空格，本轮保留该范围外改动；本轮文档单独检查。

诊断制品、原始 registry 备份、权限快照及 release 启动日志位于 `%LOCALAPPDATA%/Temp/editor-plugin-access-20261006/`。保留的一次性诊断目录不加入发行包或 workspace。

## 剩余限制

本轮恢复了默认启动路径，没有定位哪个外部操作重新加入 Low 标签；不能据此归因于 Cargo、Git、某个编辑器或防护产品，也不能保证外部重新标记后仍可运行。

当前已安装的 Markdown、SVG 和终端仍为旧 UI 契约包，另有 3 条兼容性诊断。它们需要按公开更新流程安装已构建的新包，新增权限应由用户确认；本轮不将兼容性更新与 Windows 访问拒绝的解决混作同一结果。
