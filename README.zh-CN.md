# Nanobug

<p align="center">
  <img src="crates/editor-app/assets/branding/nanobug.png" alt="Nanobug 图标" width="128" height="128" />
</p>

[English](README.md) · **简体中文**

Nanobug 是一个使用 Rust 与 GPUI 构建的原生桌面代码编辑器，优先支持 Windows。

语言支持、终端、Markdown 与图片预览，以及运行、调试和构建能力通过独立插件接入。内置插件与第三方插件使用同一套公开接口。

[文档](https://t-miracle.github.io/Editor/zh-cn/) · [快速开始](https://t-miracle.github.io/Editor/zh-cn/guide/getting-started/) · [插件 SDK](https://t-miracle.github.io/Editor/zh-cn/sdk/) · [问题反馈](https://github.com/T-miracle/Editor/issues)

## 安装

项目仍在开发中。准备下节所列开发环境后，在仓库根目录执行以下命令，生成 Windows 程序与独立插件包：

```powershell
# 构建 Release 程序、插件包与首次提供索引。
.\scripts\package-editor.ps1

# 启动打包后的编辑器。
.\dist\editor\Nanobug.exe
```

产物位于 `dist/editor/`，分发或复制程序时保留同级 `plugins/` 目录。使用已编译的程序与插件无需 Rust/Cargo。

Windows 是当前主要开发与行为验收平台，macOS/Linux 尚未完成实测或交叉构建验收。

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

源码按职责组织为编辑器应用、文档核心、平台适配、插件格式、公开协议与插件运行时六个 crate。独立插件位于 `plugins/`，构建与验收脚本位于 `scripts/`。详细入口见[维护者文档目录](docs/README.md)。

Cargo 包名保留为 `editor-app`。为保留设置、历史和已安装插件，继续使用既有 `MeEditor` 数据目录、`ME_EDITOR_*` 环境变量和已保存的命令标识。

## 插件开发

插件通过主程序的 `--plugin-cargo` 入口使用内嵌 SDK。公开能力与打包方式见[插件 SDK](https://t-miracle.github.io/Editor/zh-cn/sdk/)，宿主集成与验证入口见[维护者文档目录](docs/README.md)。

```powershell
# 构建提供当前 SDK 的宿主。
cargo build -p editor-app

# 构建运行与调试所用的独立插件包。
.\scripts\build-plugins.ps1 -HostExe .\target\debug\editor-app.exe -Packages terminal,rust,rust-debugger,run-target-example

# 验证 SDK 导出与独立插件构建。
.\scripts\verify-plugin-sdk.ps1
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
