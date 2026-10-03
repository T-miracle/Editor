# 已安装数据升级与公开 SDK 验收（#16）

当前包入口只接受协议 7 与可协商的 API。旧记录仍保存权限、全局默认和项目覆盖；界面提示更新并保留更新、卸载入口，旧组件与声明资源不参与激活。

有限历史导入位于 `plugin-runtime/src/migration/`，新包 ID 保持原样。第一次读取旧记录时，原始注册表、安装记录、宿主设置及私有文件备份在 `legacy-backup/v7/`。原文件保留，已存在目标不会被覆盖。备份仅在用户明确选择卸载并删除数据时连同旧别名数据清除；注册表备份仍保留用于元数据审计。

安装当前 SDK 包时，通过既有私有数据事务导入。旧私有文件进入当前实例的 `files`；旧快照仅按当前项目路径或已确认指向同一项目的路径别名选取，不读取其他项目的快照。每个作用域提交导入标记后不重复导入。失败候选不消耗标记或改变原备份；未打开项目在首次启用时独立导入。

宿主不解释插件私有设置内容。宿主配置与业务 `settings.json` 使用不同命名空间；仅精确的旧 `state-<16 hex>.json` 命名保留为宿主快照，其余同前缀业务文件照常导入。

## 公共验证接缝

- `Package` / `Manager`：拒绝旧协议、保留偏好和原始字节；真实 SDK WASM 重装后恢复两个项目的数据与快照，重复打开不覆盖新文件，删除数据后不自动恢复。
- 实际宿主 CLI：`--export-plugin-sdk` 导出及损坏恢复，系统临时目录中通过 `--plugin-cargo` 构建并打包；README 与声明资源随包可读。
- GPUI：已安装旧包详情显示兼容性说明，启用控件保留用户选项但不可激活，更新和卸载仍可使用；旧贡献不发布。

## 验证命令

```powershell
cargo fmt --check
cargo check --workspace
cargo test --workspace --exclude editor-app
cargo build -p editor-app
./scripts/verify-plugin-sdk.ps1
cargo test -p plugin-runtime --test sdk_distribution -- --ignored
cargo test -p plugin-runtime --test installed_migration -- --ignored
cargo test -p plugin-runtime --test data_migration -- --ignored --test-threads=1
cargo test -p editor-app -- --test-threads=1
```

所有测试使用隔离目录。旧编解码器与开发夹具的最终移除属于 #21；保留它们不表示可安装或执行旧协议包。

2026-10-03 验证：格式与工作区检查、非 UI 工作区测试通过。兼容性/别名碰撞快速回归 3 项通过；独立 SDK 包实际安装 1 项、两个项目历史导入及明确删除数据 1 项、已有迁移/失败回退/中断恢复 5 项通过。编辑器常规测试 172 项通过、21 项按前置条件忽略，包含旧包详情与当前协议 SVG 预览回归。公开 SDK 在仓库外导出、损坏修复及独立构建通过。规范与规格审查发现的业务文件筛选和历史别名碰撞问题均已补回归并修复。
