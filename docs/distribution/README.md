# 安装与分发

本目录记录维护者的安装布局、构建约束与实际验收证据。用户使用说明维护在双语站点。

- [第一期安装包规格](specs/installer-phase1.md)：Windows 按用户安装与验收；macOS/Linux 兼容入口不验收。
- [Windows 安装验收](verification/windows-installer.md)：实际产物、命令、安装生命周期和已知限制。

开发者直接使用 `cargo run` / `cargo run --release`；构建入口见仓库根 README。
当前打包直接调用工具，完整准备步骤见[原生安装器说明](../../installer/README.md)。
