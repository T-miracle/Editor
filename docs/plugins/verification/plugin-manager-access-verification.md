# Windows 插件管理器启动访问拒绝验收

状态：已完成直接 Cargo 启动修正与原生验收，2026-10-04，Windows。用户要求直接使用 `cargo run` 和 `cargo run --release`，启动不依赖外部脚本。此前的[本地日期与降序修正](plugin-log-time-order-verification.md)保持有效。

## 当前运行入口

在仓库目录执行：

```powershell
cargo run
cargo run --release
```

指定工作区或文件仍使用 Cargo 的参数分隔符：

```powershell
cargo run -- C:\path\to\project
cargo run --release -- C:\path\to\project\src\main.rs
```

此前临时复制 EXE 的启动包装已移除，README 已恢复直接 Cargo 命令。没有新增 Cargo runner、替代 target 路径或宿主启动时调用脚本的逻辑。

## 根因与复现证据

仓库根目录具有显式、可继承的 Windows Mandatory Integrity Control（MIC）Low 标签，`target` 和原有 EXE 随之继承 Low；父目录没有显式标签，按 Windows 规则属于普通 Medium。用户插件事务文件 `%APPDATA%/MeEditor/runtime-plugins/data-transactions.lock` 也属于 Medium。初始 release 真实进程完整性 RID 为 `4096`（Low），父 PowerShell 为 `8192`（Medium）；Low 进程不能向 Medium 对象写入，即使 DACL 给用户 FullControl 也会被拒绝。[Microsoft MIC 机制](https://learn.microsoft.com/en-us/windows/win32/secauthz/mandatory-integrity-control)说明该边界与进程创建规则。

- 公开 `Manager::read_registry` 最小复现三次均在约 29–42 ms 返回 `os error 5`。
- 普通读取 `registry.json` 成功，4 条安装记录完整；读写打开事务文件失败发生在申请文件锁之前。实际插件没有被卸载。
- 同一字节的诊断及 release EXE 普通复制到 Medium 目录后，进程恢复 Medium，管理器正常读取 4 条安装记录；该对照用于确认文件标签影响，不作为最终运行入口。
- 本轮修正前再次执行裸 `cargo run --release`，构建成功而原生界面仍显示“插件管理器：拒绝访问。（os error 5）”和空的已安装列表；复现日志为 `target/cargo-run-access-red.log`。

未确定最初写入仓库 Low 标签的程序或操作；不能据此归因于 Cargo、某个编辑器或防护产品。

## 本机修复及保护范围

先验证路径确为当前仓库及其子目录，并检查仓库没有目录重解析点。仅恢复本地文件的完整性标签，未更改 DACL、所有者或系统安全设置：

1. 将普通 `target/debug` 和 `target/release` 目录恢复为可继承的 Medium，读回确认已有 EXE 和新文件均为 Medium。
2. 旧原生验收夹具 `target/plugin-management-native` 的工作区、插件数据和 EXE 原本均为 Low，且仍有旧实例运行。先在夹具根保留显式、可继承的 Low 标签，再恢复仓库根的可继承 Medium。
3. 读回仓库、target、Debug/Release、两个 EXE 及夹具路径的标签；普通路径为 Medium，夹具 EXE、workspace、plugins 仍为 Low。6 个监测路径的 DACL 与修正前完全相同。

调整使用 Windows `icacls /setintegritylevel` 的继承行为，不使用 `/T`、`/reset`、`/grant` 或提升权限。[Microsoft icacls 文档](https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/icacls)及[继承传播规则](https://learn.microsoft.com/en-us/windows/win32/api/aclapi/nf-aclapi-setnamedsecurityinfow)提供依据。

仓库根修正使以后重新创建的构建目录继承 Medium，避免仅修复现有 EXE 后重建复发。完整性标签是本机文件元数据，不由 Git 保存；本次没有加入每次运行的权限调整逻辑，也没有移动、清空或重装用户插件数据。

## 验证结果

| 验证 | 实际结果 |
| --- | --- |
| 裸 `cargo run` | Debug 构建通过，33.23 秒；直接运行 `target/debug/editor-app.exe`，真实 PID 41468 的 RID 为 8192 |
| 裸 `cargo run --release` | 首次目录修正后原生验收通过；完成仓库继承修正后再次通过，缓存构建 0.97 秒，直接运行 `target/release/editor-app.exe`，真实 PID 51372 的 RID 为 8192 |
| 原生插件管理器 | 两种命令均显示 HTML 0.2.1、Rust 0.2.1、SVG 0.2.1、终端 0.7.1 已安装；详情显示卸载及重启操作，没有管理器拒绝访问红字 |
| 新 Cargo 输出目录 | 独立、无依赖临时 crate 使用全新默认 target 构建；新 EXE 自动继承 Medium，并以 Rust `OpenOptions` 成功读写打开原有事务文件，没有写入字节或获取锁 |
| 用户安装状态 | 启动前后核对 4 个插件的 manifest、digest、grants、enabled、project_enabled，全部一致 |
| 原始 release 字节 | 标签修正前后 SHA-256 一致，见下方 |
| 本地日志格式 | Debug Rust 日志显示 `2026-10-04 16:50:16`、`2026-10-04 16:50:00`、`2026-10-04 16:49:57`，最新记录在顶部，语言服务报告已就绪 |
| 正常退出 | 两种 Cargo 启动的本次验收实例均正常退出；没有操作旧夹具实例 |

原始及直接运行的 release EXE SHA-256：

```text
C4CEC70E69D20AEF9D550E2976C07FC5BD87219FB211B91B08BED5F1E3E5FD7D
```

命令输出保留在 `target/cargo-run-debug-green.log` 和 `target/cargo-run-release-final.log`。临时诊断源码、独立 Cargo 夹具及此前启动缓存已清理，没有增加正式宿主接口或测试专用能力。

本轮没有再修改 Rust 实现、协议、SDK 或插件包版本；相关 Rust 测试、格式与 workspace 检查见[日期修正验收](plugin-log-time-order-verification.md)。本轮补充实际 Cargo 构建、进程完整性读回、新构建目录继承和两种模式的原生交互验收；本次调整的文档链接及 `git diff --check` 通过。

## 实际限制

本轮验证 Windows 当前用户与现有安装环境，未实测 macOS/Linux。Debug 构建保留 3 类宿主警告及 Wasmtime/Tree-sitter 的 `LNK4217` 链接警告；Release 保留 3 类宿主警告，均未阻止构建和运行。Rust 日志仍有独立的文件监视路径警告，本次没有将所有语言工具日志判定为无警告。

对 README、插件索引和本记录检查 58 个本地链接，55 个目标存在；插件索引既有 Markdown 草案的 `plugins/markdown/docs/spec.md`、`plugins/markdown/docs/tickets/README.md` 和 `plugins/markdown/AGENTS.md` 仍缺失，未纳入本次启动修正。

为保留仍在运行的旧夹具，没有对整个仓库执行 `cargo clean`；全新默认 Cargo target 的验证覆盖新输出继承。源目录再次被外部操作标为 Low 时仍须定位该外部来源，不能由应用静默提升自己的权限。
