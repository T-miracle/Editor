# Nanobug 品牌与兼容标识

日期：2026-10-08。

产品名称由 Me Editor 改为 Nanobug。编辑器标题栏、原生窗口标题、LSP 客户端标识、Windows 文件属性、文档站点和根 README 使用新名称。打包脚本生成 `dist/editor/Nanobug.exe`；Cargo 包及开发构建仍为 `editor-app`，以保留现有开发和 SDK 构建入口。

原始图标保存在 `crates/editor-app/assets/branding/nanobug.png`，与用户提供的 PNG 字节一致。相邻 ICO 保留透明度并包含 16、24、32、48、64、128、256 像素图层；Windows 资源 ID 1 与 GPUI 上游的原生图标加载接口对应。站点头部及 favicon 使用同图的缩小版本与 ICO。

`README.md` 为英文正本，`README.zh-CN.md` 为中文译文；两份以相对路径互链并显示同一个图标，适用于 GitHub 分支页面与本地检出。

本次产品改名保留以下兼容标识，避免将已有配置或插件当作新的空安装：

- `MeEditor` 用户数据、历史、会话、快捷键及 SDK 缓存目录。
- `ME_EDITOR_*` 环境变量、`me_editor::` 快捷键动作名与既有插件协议标识。
- 现有 GitHub 仓库地址和 `/Editor/` 文档站点路径；线上链接随仓库实际地址维护。

历史需求、工单和验收中的旧产品名称保留为当时记录。上述兼容标识若另行更改，需要独立的数据迁移与消费者验证。

## 本地工作区验证

以下是初次验证时完整本地工作区的结果，包含当时已有的其他开发改动；实际提交候选另行在隔离副本验证，不能将两者混为同一个测试对象。

- `cargo fmt --check`、`cargo check --workspace --locked` 和 `git diff --check` 通过。
- `cargo test --workspace --exclude editor-app --locked -j 4`：204 项通过，173 项 ignored；没有将跳过的 WASM 夹具测试计为通过。
- `cargo test -p editor-app ui::assets::tests --locked -- --test-threads=1`：4 项通过。
- `cargo build -p editor-app --locked` 通过；Windows 文件属性中的产品名、文件说明为 Nanobug，原始文件名为 `Nanobug.exe`，版本与 Cargo 的 `0.1.0` 一致。
- 原生窗口标题、标题栏图标及深浅主题下的可见效果已检查，验收后恢复原有浅色主题并关闭验收窗口。
- `website/` 的 `npm run build` 通过，构建 52 个页面并通过全部 17 项文档、链接和搜索测试。
- 中英文 README 本地链接、图标路径和原图字节一致性已检查；打包脚本语法解析通过。

首次验证遇到已有 Rust 构建缓存与当前源码不一致，以及普通终端未加载完整 MSVC 环境的问题。仅清理对应包的生成缓存，并在验证进程中加载已安装的 Visual Studio 开发环境后重跑成功，未安装或修改全局工具链。

完整 Release 分发打包与 ignored WASM 夹具测试未在本次执行；以上记录不代表已经发布完整发行包。GitHub 仓库地址保持不变，线上 README 与站点随普通推送和部署更新。

## 提交候选验证

从 `17be755` 加本次暂存的 31 个品牌文件导出独立文件树，使用独立 Rust 构建目录验证。其他工作区的插件实现、协作约定和文档目录迁移未纳入；README 链接使用已经提交的维护者入口。

- `cargo fmt --check`、`cargo check --workspace --locked -j 4`、`cargo build -p editor-app --locked -j 4` 通过。
- 非 UI workspace 测试：196 项通过、167 项 ignored；相关应用资源测试：4 项通过。
- 站点构建与全部 17 项测试通过，双语 README、文档目录和品牌说明的候选文件链接检查通过。
- 候选原生程序的 Nanobug 窗口标题及图标已检查；Windows 产品名、文件说明和原始文件名与品牌约定一致。验收窗口正常关闭。
