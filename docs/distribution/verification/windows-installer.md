# Windows 第一期安装包验收

日期：2026-10-08。状态：Windows 第一期安装、运行中保护、原位覆盖修复与卸载均通过本机验收。
归档说明：下列命令记录第一期当时的执行方式。随后用户要求移出全部脚本，旧脚本已在本地
Codex 工作区存档；这些命令不再作为当前入口。当前打包直接调用工具，见
[原生安装器说明](../../../installer/README.md)。本次脚本迁移没有重新构建或重验安装产物。
范围依据：[第一期规格](../specs/installer-phase1.md)。macOS/Linux 本期不构建、不运行、不验收。

## 环境与产物

- Microsoft Windows 11 专业版，10.0.26200，x64；Rust `x86_64-pc-windows-msvc`，已安装 `wasm32-wasip2`。
- 本机已有 Visual Studio C++ Build Tools/Windows SDK，仅向构建进程导入开发环境，不改全局 PATH。
- Inno Setup 6.7.3 从[官方下载入口](https://jrsoftware.org/isdl.php)获取，Authenticode 为 Valid，
  发布者为 Pyrsys B.V.；用户明确批准仅安装到临时目录。任务结束清理临时编译器安装。
- 产物基于当前工作区（包含原有未提交变更），不是纯 HEAD 构建；本任务不自动提交、推送或发布。
- 最终文件：`dist/installers/Nanobug-Setup-0.1.0-x64.exe`，23,234,316 字节。
- SHA-256：`025f09d4129fc71f888e07e458e8a5071eeedacaaade11aec2df33f8fe6dd2b9`。
- 对应校验文件：`dist/installers/Nanobug-Setup-0.1.0-x64.exe.sha256`。
- runtime 文件位于 `dist/editor/`；包含主程序、图标、10 个独立插件 ZIP、首次提供索引和许可文本。

## 构建与自动检查

从仓库根执行，Inno 路径由本次临时安装提供：

```powershell
# 导入本机开发工具环境后构建全部 Windows 分发文件。
.\scripts\package-editor.ps1 -Compiler '<临时目录>\Inno Setup 6\ISCC.exe'

# 资源查找修正后重建主程序与安装器；插件 SDK/包的内容未受此修正影响。
cargo build -p editor-app --release
Copy-Item -LiteralPath .\target\release\editor-app.exe -Destination .\dist\editor\Nanobug.exe
.\scripts\build-installer.ps1 -Compiler '<临时目录>\Inno Setup 6\ISCC.exe'

# 格式、非 UI 测试、应用针对性测试和 workspace 编译检查。
cargo fmt --check
cargo test --workspace --exclude editor-app
cargo test -p editor-app app::distribution
cargo check --workspace

# 双语站点的内容、链接与搜索门禁。
Push-Location website
npm test
Pop-Location
```

结果：格式与编译通过；非 UI workspace **204 passed、173 ignored**，忽略项未执行，
不计入通过。分发模块 **3 passed、0 ignored**：Windows mutex 最后一个 GUI 生命周期、
直接运行后备位置、安装包与源码旧资源互斥选择。站点 **17 passed**。
本期没有修改插件公开契约或 SDK 导出实现，没有扩大为全部插件 WASM/原生功能验收。
编译仍有工作区既有未使用代码、可见性和链接器警告，未为本任务顺手修改。

日志在 `target/windows-package-phase1.log`、`target/windows-installer-compile.log`、
`target/installer-final-host-build.log`、`target/installer-app-tests.log`、
`target/installer-workspace-tests.log`、`target/installer-workspace-check.log`。

## 实际安装生命周期

安装前确认本账户没有 Nanobug 卸载项和快捷方式，脚本再次强制检查；不覆盖用户已有安装。
最终测试目录为 `%TEMP%/nanobug-install-verification-8bab448f5aff45a08f27f895a29e67bb/Nanobug`。
用户目录中的唯一测试哨兵用来验证保留数据，结束后只清理这个测试文件。

```powershell
# 全程使用真正的最终 Setup；每一阶段保存报告与安装器日志。
$setup = '.\dist\installers\Nanobug-Setup-0.1.0-x64.exe'
.\scripts\verify-windows-installer.ps1 -Installer $setup -Phase Install

# 此处先正常启动安装目录中的 Nanobug.exe，并保持原生窗口打开。
.\scripts\verify-windows-installer.ps1 -Installer $setup -Phase WhileRunning

# 此处关闭全部验收程序窗口后继续。
.\scripts\verify-windows-installer.ps1 -Installer $setup -Phase Upgrade
.\scripts\verify-windows-installer.ps1 -Installer $setup -Phase Uninstall
```

| 项目 | 实际检查 | 结果 |
| --- | --- | --- |
| 安装 | 非管理员按用户安装；逐个对比全部 runtime 文件 SHA-256；HKCU 名称 Nanobug、安装位置正确 | 通过 |
| 快捷方式 | 开始菜单和测试时勾选的桌面快捷方式均指向安装目录主程序 | 通过 |
| 原生启动 | 标题 Nanobug、原生品牌图标、编辑器窗口和插件管理界面可见 | 通过 |
| 本地插件资源 | 插件市场列出发行包对应的 10 项资源，旧源码树包不再重复出现 | 通过 |
| CLI 独立启动 | 安装后的 `Nanobug.exe --export-plugin-sdk <临时目录>/exported-sdk` 正常导出 Cargo.toml 与 SDK | 通过 |
| 运行中保护 | 真正 GUI 保持打开时 Setup 和卸载器均取消，日志明确“Nanobug 当前正在运行”，runtime 保持完整 | 通过 |
| 原位覆盖修复 | 同版本安装器恢复故意替换的图标并核对全部文件；原目录不变 | 通过 |
| 卸载 | 移除主程序、plugins、快捷方式与卸载注册项；用户目录哨兵不变 | 通过 |

阶段报告：`target/windows-installer-verification.json`。安装器原生日志位于报告 root 下的
`Install.log`、`WhileRunning.log`、`blocked-uninstall.log`、`Upgrade.log`、`Uninstall.log`。
早期一次名称断言发现默认卸载项会加版本后缀，已明确设置 UninstallDisplayName；该隔离安装
已清理。另一次原生列表检查发现旧源码插件混入，修正资源选择并重新构建、安装、验收最终产物。

## 验收限制

- 原位覆盖使用当前 0.1.0 安装器，验证文件替换与保留目录，不声称跨版本数据迁移通过。
- 未测试没有开发工具的干净 Windows 虚拟机、其他 Windows 版本、ARM64 或安装中断回滚。
- Nanobug 安装器未签名；本期不包含代码签名或 SmartScreen 信誉验收。
- 原生启动沿用本账户既有插件设置，能看到既有语言服务状态提示；本期不更改权限、不安装
  或升级用户插件，不以安装验收宣称所有插件功能健康。
- macOS/Linux 只有兼容打包代码、布局、图标与使用入口，未执行这两端的任何构建或安装校验。
