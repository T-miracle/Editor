# 插件故障恢复验证（工单 12 / GitHub #13）

## 交付

- WASM 每次调用同时使用 fuel 与 epoch 时限；组件线性内存累计限制 256 MiB，超限实例立即退休。
- 管理页关联插件、作用域、操作展示有界诊断，提供独立“重启插件”。失败请求与资源随实例撤销，保留最后安全快照及私有数据。
- LSP 首次失败后等待 1 秒，再次失败后等待 4 秒，第三次失败暂停自动启动；同一连接成功工作满 60 秒才恢复预算，离线时间不计入。
- 迟到文档操作在传输层之前被拒绝，不消耗连接故障预算。诊断与重启在现有后台执行/发布通道完成。
- 独立 SDK 示例提升至 0.10.0，真实死循环和超内存命令验证通用宿主策略；SDK 导出 `FAULTS.md`。

## 验证

```powershell
cargo fmt --check
cargo fmt --manifest-path plugins/capability-example/Cargo.toml --check
cargo check --workspace
cargo test --workspace --exclude editor-app
cargo test -p editor-app -- --test-threads=1
cargo build -p editor-app
./scripts/build-capability-example.ps1
cargo test -p plugin-runtime --test fault_recovery -- --ignored --test-threads=1
cargo test -p editor-app crashing_lsp_stops_retrying -- --ignored --test-threads=1
cargo test -p editor-app retiring_provider_cancels_request -- --ignored --test-threads=1
cargo test -p editor-app native_restart_recovers_a_fault -- --ignored --test-threads=1
```

WASM 验收覆盖死循环终止、内存超限、同宿主健康插件不受影响、待执行请求失效、资源归零及独立重启。
删除隔离安装目录的组件文件模拟恢复失败，确认明确报错且私有数据保留；恢复文件后再次重启并从 guest 读回原数据。
真实 LSP 子进程配合可控时钟验证三次失败上限、长时间离线后短命成功连接不重置预算、手动恢复和旧连接退休。
原生 GPUI 验收在插件死循环期间输入未保存文字，再点击“重启插件”，确认编辑内容保持且磁盘未被擅自修改。

全工作区非 UI 测试及编辑器 167 项普通测试通过；上述四项专门验收通过。规范轴与规格轴复审通过。
已有 Windows wasmtime/tree-sitter 链接告警仍存在。本工单不宣称原生进程隔离可约束所有系统资源，
也不承诺撤销外部副作用；正式插件迁移与完整桌面验收仍属后续工单。
