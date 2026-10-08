# Nanobug 第一期安装包规格

状态：Windows 第一期实施与本机安装验收完成；macOS/Linux 仅交付兼容打包入口，本期不构建、不安装验收。

## 已确认范围

- 本地运行、调试继续直接启动，不经过安装器。
- Windows 采用 Inno Setup，分发单个 Setup 下载文件，安装后落为常规目录中的程序、插件包与许可文件。
- macOS 采用 `.app` + `.dmg`，另提供 `.pkg`；Linux 提供 `.deb` / `.rpm`。必须使用对应系统与架构的原生产物。
- 沿用 Nanobug 名称和现有品牌图片；保留 `MeEditor` 数据路径及持久化标识。
- 本期不增加自动更新、在线市场、文件关联、全局 PATH 修改或 macOS/Linux 原生验收。

## Windows 实现

默认安装到 `%LOCALAPPDATA%\Programs\Nanobug`，权限为当前用户，不要求管理员。
稳定 AppId 为 `Nanobug.Editor`，卸载注册项在 HKCU。开始菜单快捷方式默认创建，桌面快捷方式可选。
快捷方式的工作目录为用户 Documents，避免将安装目录作为项目或写入位置。

安装内容为 `Nanobug.exe`、`Nanobug.ico`、`plugins/` 与 `licenses/`。
插件仍是独立 ZIP；`plugins/bundle-defaults.json` 与其中记录的包 SHA-256 必须一致。
许可文本跟随运行文件分发，插件包内的许可照常保留。不打包 Rust/Cargo、SDK 源码、项目源码或用户数据。

GUI 启动时保留 `Local\Nanobug.Installer.Running` 命名 mutex，所有窗口所在进程退出后才释放。
安装器和卸载器均检查该 mutex；不强制关闭程序，避免丢失未保存文档。
CLI SDK 导出、插件编译和运行清理入口不持有 GUI mutex。

覆盖安装保留原路径；现有同名运行文件被新产物替换。已安装插件的权限、选择和私有数据留在
既有用户目录中，安装器不会执行插件或替用户批准权限。卸载只移除安装器拥有的程序文件、快捷方式和注册项。
新版本不再携带的旧插件 ZIP 可能继续留在安装目录；变更默认包清单时需显式设计退休清理，不能使用通配符清空安装目录。

当前入口为 `installer/windows/nanobug.iss`；准备运行文件后直接调用 ISCC，版本从 Cargo metadata
读取，输出到 `dist/installers/`。开发直接运行使用 Cargo，不经过安装器。

2026-10-08 用户明确要求移出脚本目录并禁止打包调用这些脚本，17 个原脚本已完整移至本地
`C:/Users/Tmiracle/.codex/workspaces/Nanobug/scripts`。该位置仅作存档，仓库与 CI 不依赖它，
不通过包装器或链接间接调用。当前准备与构建命令见[原生安装器说明](../../../installer/README.md)。

安装语言为英文和简体中文。简中翻译保留上游注释，来自
[kira-96/Inno-Setup-Chinese-Simplified-Translation](https://github.com/kira-96/Inno-Setup-Chinese-Simplified-Translation)，
固定来源提交 `1ff90acc4ed4aee82b1cda43253243deee3daed4`，MIT 许可收录在
`THIRD_PARTY_LICENSES/inno-chinese-simplified-LICENSE`。

## macOS/Linux 兼容入口（不验收）

原 Python 包装器已随其他脚本移出并存档，不再作为兼容打包入口。对应平台直接使用 hdiutil、
pkgbuild、dpkg-deb、rpmbuild，准备步骤见原生安装器说明；不在 Windows 上伪装 ELF/Mach-O，
按真实原生二进制确定架构。

payload 根目录包含 `Nanobug` 原生可执行文件、`plugins/bundle-defaults.json`、对应 ZIP 和 `licenses/`。
原生工具链、GPUI 平台依赖、WASM target 及可用插件须在对应机器准备。现有 Windows 专属 Rust
调试桥依赖声明不因增加安装入口而获得 macOS/Linux 支持；原生端先提供适合该平台的插件集合。
不删除现有插件能力，也不在宿主增加按插件 ID 判断的平台特例。

macOS 布局：`Nanobug.app/Contents/MacOS/Nanobug`，插件与许可位于 `Contents/Resources/`。
Info.plist 使用 `io.github.t-miracle.nanobug`，图标为基于品牌 PNG 生成的 ICNS。
Info.plist 的最低系统版本必须与原生构建部署目标一致。DMG 用 hdiutil 创建，带 Applications 链接；
PKG 用 pkgbuild 创建并安装到 `/Applications`。本入口输出未签名产物；Developer ID、WASM JIT
所需签名授权、公证与装订属于下一阶段发布准备，不宣称已满足 Gatekeeper 分发要求。

Linux 布局：`/usr/lib/nanobug/nanobug` 与相邻 `plugins/`、`licenses/`；
`/usr/bin/nanobug` 为启动链接，另安装 `.desktop` 和 256px PNG 图标。
DEB 使用 dpkg-shlibdeps 计算原生共享库依赖，再由 dpkg-deb 以 root 所有权打包。
RPM 使用 rpmbuild 的 ELF 依赖扫描，保留已优化的主程序，安装、升级、删除由包管理器管理。
不额外创建用户数据或卸载脚本，不将宿主机工具链装入安装包。

资源查找由 `app/distribution.rs` 统一提供给本地插件列表和首次提供索引：存在安装资源目录时，
只读取该目录，不混入源码树中的旧包；没有安装资源目录时才使用源码工作区的 `dist/plugins`
作为直接 Cargo 启动的后备位置。测试夹具的隔离目录维持原契约。

## Windows 验收方式

第一期原验收脚本按 Install、WhileRunning、Upgrade、Uninstall 四阶段执行，现只在 Codex
工作区存档。下列内容保留当时验收边界，不作为新的脚本调用入口。
初始阶段拒绝已有 Nanobug 卸载项或快捷方式，使用随机临时安装目录；当前用户目录写入唯一测试
哨兵文件，确认各阶段不会删除或改写。测试结束只移除这个测试文件，不删除 MeEditor 目录。

Install 逐个核对所有 runtime 文件哈希、开始菜单与桌面快捷方式和 HKCU 注册信息。
WhileRunning 要求真正的安装后 GUI 打开，验证安装和卸载均停止；关闭 GUI 后继续 Upgrade。
Upgrade 人为将本次安装拥有的图标替换为旧内容，再使用同版本安装器进行原位覆盖修复并核对
全部文件。此项验证覆盖路径与文件替换，**不等同于跨版本数据迁移验收**。
Uninstall 检查程序、插件、快捷方式和注册项已移除，私有数据哨兵仍完整。

命令、日志与结果见[验收记录](../verification/windows-installer.md)。本地 Windows 环境无法证明
其他 Windows 版本、未安装开发工具的干净机器、不同 CPU、安装中断回滚或签名信誉行为。
