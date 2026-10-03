# 私有数据迁移验证（工单 13 / GitHub #14）

## 交付

- 清单声明 `data_format` 与可选迁移钩子；`storage.migration` 1.0 提供携带版本及不透明快照的回调。
- 准备令牌保留旧实例，提交重新取得最终文件和快照。迁移只能访问隔离副本，新版本初始化只接收迁移后的快照。
- 各工作区持久化独立数据版本；未迁移工作区在启用前转换，项目单独启用保留真实激活意图。
- 候选激活成功后，按恢复日志 → 数据目录交换 → 注册表 → 完成标记的顺序提交，再清理旧副本。
- 未提交日志在启动前回退，已提交日志只清理；恢复失败保留副本并阻止实例启动，私有数据以外的副作用不自动回滚。
- 事务锁保护注册表读取；WASM 私有数据运行时所有权防止另一宿主运行混合版本，取得所有权后重新读取元数据。纯声明式依赖管理器仍可并行。
- 独立 SDK 夹具升级至 0.11.0，公开 SDK 导出 `MIGRATION.md`。服务、语言及多种 UI 贡献的统一切换由 #15 继续完成。

## 验证

```powershell
cargo fmt --check
cargo fmt --manifest-path plugins/capability-example/Cargo.toml --check
cargo check --workspace
cargo test --workspace --exclude editor-app
cargo test -p editor-app -- --test-threads=1
cargo build -p editor-app
./scripts/build-capability-example.ps1
cargo test -p plugin-runtime --test data_migration -- --ignored --test-threads=1
```

通过公共 Package/Manager 及独立 SDK guest 验证：隔离 v1→v2 转换、准备后旧写入不丢、
迁移拒绝、激活失败、首次安装失败、目录交换后取消恢复、旧工作区项目启用、过期元数据所有权刷新。
子进程在 Committing/Committed 真实进度边界强退，分别验证恢复旧版本或保留新版本；
另以真实文件系统故障阻止恢复，修复后再次打开确认保留数据可恢复。

结果：格式、工作区编译/非 UI 测试、主程序和独立 SDK 构建均通过；编辑器普通测试 167 项通过，
5 项迁移测试通过（含一个子进程入口）。随后补充恢复文件存在性检查，并单独重跑真实中断恢复验收通过：
已提交数据目录缺失时报告失败并保留备份，恢复目录后正确启用新版本。规范轴与规格轴复审通过。
已有 Windows wasmtime/tree-sitter 链接告警及两处 dead_code 告警仍存在。
