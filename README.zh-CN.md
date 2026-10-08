# Nanobug

<p align="center">
  <img src="crates/editor-app/assets/branding/nanobug.png" alt="Nanobug 图标" width="128" height="128" />
</p>

[English](README.md) · **简体中文**

Nanobug 是一个使用 Rust 与 GPUI 构建的原生桌面代码编辑器，优先支持 Windows。

语言支持、终端、Markdown 与图片预览，以及运行、调试和构建能力通过独立插件接入。内置插件与第三方插件使用同一套公开接口。

[文档](https://t-miracle.github.io/Editor/zh-cn/) · [快速开始](https://t-miracle.github.io/Editor/zh-cn/guide/getting-started/) · [插件 SDK](https://t-miracle.github.io/Editor/zh-cn/sdk/) · [问题反馈](https://github.com/T-miracle/Editor/issues)

## 安装

项目仍在开发中。Windows 用户运行 `Nanobug-Setup-<版本>-x64.exe`，按当前用户安装。安装器创建开始菜单入口，提供可选桌面快捷方式，并注册卸载程序。程序与插件以独立文件安装到 `%LOCALAPPDATA%\Programs\Nanobug`，使用时无需 Rust/Cargo。

更新或卸载前先正常退出 Nanobug。既有 `MeEditor` 目录中的设置、历史与已安装插件数据会保留。Windows 安装界面提供英文和简体中文。

准备下节所列开发环境及 [Inno Setup](https://jrsoftware.org/isdl.php) 后，直接构建宿主：

```powershell
# 构建原生 Release 主程序；开发时仍可直接启动。
cargo build -p editor-app --release
cargo run --release
```

按[原生打包说明](installer/README.md)准备程序、插件 ZIP、首次提供索引、图标与许可文件，然后从仓库根直接编译 Windows 安装器：

```powershell
# 读取真实宿主版本，直接调用原生安装器编译工具。
$version = ((cargo metadata --no-deps --format-version 1 | ConvertFrom-Json).packages |
    Where-Object name -eq 'editor-app').version
$payload = (Resolve-Path .\dist\editor).Path
$output = Join-Path $PWD 'dist/installers'
ISCC.exe "/DPayloadDir=$payload" "/DInstallerDir=$output" "/DAppVersion=$version" .\installer\windows\nanobug.iss
```

安装器输出到 `dist/installers/`，准备好的运行文件保留在 `dist/editor/`。ISCC 未加入 PATH 时指定编译工具的完整路径；便携复制时保留同级 `plugins/` 与 `licenses/` 目录。

仓库不保留辅助脚本目录。打包直接调用 Cargo、宿主公开 CLI、归档工具和原生安装器工具；禁止调用移入 Codex 工作区的旧脚本，包括通过包装器间接调用。

第一期已验证 Windows 安装、原位覆盖修复、运行中保护和卸载。macOS 应用包、DMG/PKG 与 Linux DEB/RPM 改为直接使用系统工具，准备步骤与命令见[原生打包说明](installer/README.md)。**本期不构建、不验收这两端的安装行为**；签名、公证与平台专属插件依赖另行准备。

## 开发 Nanobug

Windows 开发环境需要 Rust MSVC 工具链（Rust 1.95 或更新版本）、C++ 构建工具与 Windows SDK。构建插件及完整分发目录还需已安装的 `wasm32-wasip2` 目标。版本与依赖约束见 [Cargo.toml](Cargo.toml)、[rust-toolchain.toml](rust-toolchain.toml) 和 [Cargo.lock](Cargo.lock)。

在仓库根目录运行：

```powershell
# 启动 Debug 开发版本。
cargo run

# 启动优化后的 Release 版本。
cargo run --release

# 打开指定工作区；也可传入文件路径。
cargo run --release -- C:\path\to\project
```

Debug 构建未经优化，窗口拖动与编辑响应可能较慢，日常体验建议使用 Release。

源码按职责组织为编辑器应用、文档核心、平台适配、插件格式、公开协议与插件运行时六个 crate。独立插件位于 `plugins/`，原生打包配置与步骤位于 `installer/`。详细入口见[维护者文档目录](docs/README.md)。

Cargo 包名保留为 `editor-app`。为保留设置、历史和已安装插件，继续使用既有 `MeEditor` 数据目录、`ME_EDITOR_*` 环境变量和已保存的命令标识。

## 插件开发

插件通过主程序的 `--plugin-cargo` 入口使用内嵌 SDK。公开能力与打包方式见[插件 SDK](https://t-miracle.github.io/Editor/zh-cn/sdk/)，宿主集成与验证入口见[维护者文档目录](docs/README.md)。

```powershell
# 构建提供当前 SDK 的宿主。
cargo build -p editor-app

# 通过真实宿主的公开 SDK 入口独立构建组件。
.\target\debug\editor-app.exe --plugin-cargo plugins/terminal/Cargo.toml build --target wasm32-wasip2 --release

# 直接导出内嵌 SDK，供检查或外部工具链使用。
.\target\debug\editor-app.exe --export-plugin-sdk .\target\sdk-export
```

在插件管理中安装或更新生成的包，并批准声明的权限。运行与调试的使用方式见[用户指南](https://t-miracle.github.io/Editor/zh-cn/guide/run-debug-build/)；Rust 调试器的依赖准备与授权范围见[插件说明](plugins/rust-debugger/README.md)。

## 参与贡献

欢迎通过 [GitHub Issues](https://github.com/T-miracle/Editor/issues) 反馈问题或提出建议。修改前请从[维护者文档目录](docs/README.md)查找对应规格与验收要求。

Rust 代码变更的基本检查：

```powershell
# 检查 Rust 格式。
cargo fmt --check

# 运行非 UI workspace 测试。
cargo test --workspace --exclude editor-app

# 检查整个 workspace 的编译。
cargo check --workspace
```

修改 `editor-app` 时还需运行相关 UI 测试与原生交互验收；实际 WASM 包的 ignored 测试需先构建夹具，再显式执行。纯文档变更检查链接、路径及 `git diff --check`。

面向读者的使用说明与 SDK 正文维护在 `website/`，站点构建方式见 [website/README.md](website/README.md)。项目规格、工单与验收记录维护在 `docs/`。本中文 README 与[英文正本](README.md)同步维护。

## 许可

工作区包在 [Cargo.toml](Cargo.toml) 中声明使用 MIT 许可。第三方依赖及插件携带的 grammar、图标与其他资源遵循各自许可；具体来源与许可文本见对应插件目录及分发包。
