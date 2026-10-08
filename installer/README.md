# 原生安装器与直接打包

2026-10-08 起，构建、插件归档与发行打包直接使用工具命令。仓库中的 `scripts/` 已移除；旧
PowerShell/Python 脚本只在本机 Codex 工作区存档，打包不得调用这些脚本或通过包装器间接调用。
本规则也适用于后续 CI 打包；不将本机 Codex 路径写入构建依赖。

## Windows

运行 `cargo build -p editor-app --release` 后，将 `target/release/editor-app.exe` 复制为
`dist/editor/Nanobug.exe`；复制品牌 ICO 到 `dist/editor/Nanobug.ico`，第三方许可到
`dist/editor/licenses/`。通过主程序 `--plugin-package` 按各项目的 `nanobug-plugin.json` 独立构建
并自动生成 `dist/editor/plugins/<id>-<version>.zip`。不要把编译器、宿主源码或脚本放入运行目录。

有组件的默认包为 terminal、example、svg、rust、rust-debugger、markdown；组件 Cargo 名称分别
为 terminal-guest、example-guest、svg-guest、rust-language-guest、rust-debugger-guest、markdown-guest。
资源包为 toml、html、javascript、run-target-example。每包保留根目录 `manifest.json` 与 `README.md`，
存在时还包含 `plugin.toml`、`icons.json`、grammar、queries、icons、run-targets 等运行资源。
只收录清单引用资源，不归档 src、Cargo 缓存或 target 目录。

```powershell
# 项目描述负责 SDK、原生桥、资源、摘要和 ZIP；显式指定安装目录中的插件输出位置。
$hostExe = (Resolve-Path .\target\release\editor-app.exe).Path
& $hostExe --plugin-package plugins/terminal plugins/example plugins/svg plugins/rust plugins/rust-debugger plugins/markdown plugins/toml plugins/html plugins/javascript plugins/run-target-example --output dist/editor/plugins
if ($LASTEXITCODE -ne 0) { throw 'Plugin packaging failed' }
```

宿主把 WASM 映射到清单规定的包内路径，原生桥摘要写入分发清单，不修改源占位。
terminal 项目描述收录终端核心与 VTE 许可；markdown 将 `src/pulldown-cmark-LICENSE`
映射到 `licenses/pulldown-cmark-LICENSE`。完整包校验通过后才原子替换 ZIP。

自动 ZIP 的包根不会多套一层文件夹。首次提供索引 `plugins/bundle-defaults.json` 使用 version 1；
packages 中 Markdown 项的 file 必须匹配本次实际生成的 `markdown-<清单版本>.zip`，
file_extensions 为 `["md", "markdown"]`，sha256 为最终 ZIP 的小写 SHA-256。
所有 ZIP/资源与索引准备好后，从仓库根执行：

```powershell
# 版本来自 Cargo，不通过旧打包脚本读取；ISCC 接收准备好的运行目录。
$version = ((cargo metadata --no-deps --format-version 1 | ConvertFrom-Json).packages |
    Where-Object name -eq 'editor-app').version
$payload = (Resolve-Path .\dist\editor).Path
$output = Join-Path $PWD 'dist/installers'
ISCC.exe "/DPayloadDir=$payload" "/DInstallerDir=$output" "/DAppVersion=$version" .\installer\windows\nanobug.iss
if ($LASTEXITCODE -ne 0) { throw 'Installer compilation failed' }

# 校验文件直接来自最终产物。
$setup = Join-Path $output "Nanobug-Setup-$version-x64.exe"
$digest = (Get-FileHash -LiteralPath $setup -Algorithm SHA256).Hash.ToLowerInvariant()
"$digest  $([IO.Path]::GetFileName($setup))" | Set-Content -LiteralPath "$setup.sha256" -Encoding ascii
```

Inno Setup 需事先准备，未加入 PATH 时直接指定 ISCC.exe 完整路径。x64 是第一期验收架构；
检查原生 EXE 架构和首次提供索引的包摘要后再发行。安装配置保留按用户安装、运行中 mutex
保护、双语向导与用户数据保留行为，不包含外部程序或脚本执行项。

## macOS/Linux（本期未验收）

这些系统必须先准备对应架构的原生 Nanobug 与适合平台的插件包。旧 Python 包装器不作为
当前入口；用以下原生工具直接打包，不能用 Windows EXE 替代 Mach-O/ELF。

- macOS：准备 `Nanobug.app/Contents/MacOS/Nanobug`、`Contents/Resources/plugins` 与 licenses；
  将品牌 ICNS 复制到 Resources。Info.plist 保留产品名称、版本、bundle ID
  `io.github.t-miracle.nanobug`、CFBundleExecutable Nanobug 和实际最低系统版本。
  DMG 的暂存目录包含 `.app` 和指向 `/Applications` 的链接。
- Linux：程序和相邻 plugins/licenses 放到 `/usr/lib/nanobug`，`/usr/bin/nanobug` 链接到
  `../lib/nanobug/nanobug`；补充 `.desktop` 与 256px 品牌 PNG。DEB 的 control 填写实际
  version/architecture/maintainer，使用 dpkg-shlibdeps 计算依赖；RPM 用实际原生 Requires。

```bash
# macOS：直接调用系统工具；暂存 .app 和 Applications 链接需事先准备。
hdiutil create -volname Nanobug -srcfolder dist/macos-stage -format UDZO dist/installers/Nanobug.dmg
pkgbuild --component dist/macos-stage/Nanobug.app --install-location /Applications dist/installers/Nanobug.pkg

# Debian 系：准备完整包根及 DEBIAN/control 后直接打包。
dpkg-deb --build --root-owner-group dist/deb-root dist/installers/Nanobug.deb

# RPM 系：准备原生 spec 与 SOURCES 后直接打包。
rpmbuild -bb nanobug.spec
```

macOS 签名、公证、平台依赖和最低系统版本另行准备；本期继续不执行 macOS/Linux 构建或
安装验证。第一期 Windows 验收记录保留当时命令，不能用历史脚本命令替代当前流程。
