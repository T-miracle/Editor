# Rust 插件

版本 0.2.0 使用协议 7。语言识别、Tree-sitter 高亮和图标由资源声明提供；独立 WASM 钩子负责 Rust Analyzer 的 SDK 配置和项目发现，宿主不识别 Rust 名称。

Rust Analyzer 服务使用包内固定声明：优先检查 HOME 下 Scoop、VS Code 和 VS Code Insiders 的扩展目录，再检查系统 PATH。候选程序必须通过 `--version` 探测；缺少可用程序时报告服务准备失败。仅按声明路径查找工具，不自动安装工具，也不申请任意进程执行权限。

插件设置中的“Rust Analyzer 可执行文件”支持用户设置与明确保存的项目覆盖。默认自动查找；保存绝对路径后只使用该文件，错误不会被自动回退掩盖。使用重置恢复自动查找。

语言钩子通过 `workspace.files` 1.1 查找当前工作区内的 `Cargo.toml` 与同目录 `manifest.json`，保留根 Cargo 项目，排除 `target`、`vendor`、`node_modules` 和 `.git` 并遵守忽略规则。不可读的无关目录不会停用语言分析。Windows 路径按 Rust Analyzer 的文档 URI 规则规范化，避免未保存内容被误判为外部库。

通过 `host.sdk` 获取宿主发布的 SDK 与 Cargo 配置路径；`cargo.configPath`、实验性原生诊断及完整和分节 `rust-analyzer` 配置由本插件提供。无需在独立插件项目写入 `.cargo`、复制 SDK 或引用宿主业务源码。服务声明请求 `serverStatusNotification`，等待 `experimental/serverStatus` 的 `/quiescent` 为 `true`，最多 120 秒。

使用已构建宿主打包：

```powershell
./scripts/build-plugins.ps1 -HostExe ./target/debug/editor-app.exe -Packages rust,toml,html,javascript
```

单独编译时使用公开 SDK 入口：

```powershell
editor-app.exe --plugin-cargo plugins/rust/Cargo.toml build --target wasm32-wasip2 --release
```

打包包含 `rust.wasm`、清单、README、grammar、查询、图标和许可证；不包含 Rust 源码、Cargo 缓存或 SDK 副本。安装或更新后已打开文件重新选择 provider；停用、切换 provider 或卸载会释放本插件的服务。
