# 插件服务契约验证记录

状态：已完成（2026-10-03 状态汇总）。本记录保留该阶段实际执行的结果与限制；后续迁移、旧协议删除及最终交付以[完整契约验收](plugin-api-contract-verification.md)为准。20 张工单均已提交、推送并关闭；早期正文中的“尚未推送”“后续工单”是当时的历史状态。

对应工单：[#12](https://github.com/T-miracle/Editor/issues/12)，规格 T20。
基线 `43923c84329bf5c7b5629c207feaf8389f290b21`。

## 交付

- `plugin.services` 1.0 提供有界、版本化的参数/结果 schema；消费者依赖契约而非插件 ID。
- 管理器按作用域发现服务，唯一提供者自动解析，多个提供者由原生插件设置选择；明确区分用户默认、项目覆盖和自动选择。
- 必需依赖在激活前校验；启动不依赖插件名称排序，可选依赖可降级。引用绑定提供者实例和单调选择版本，切换后不能复活。
- 编辑器与服务共用完成门、截止时间、取消和终态规则。调用链保留来源及收缩后的权限，循环和深度有界。
- 实例生命周期令牌在 stop/Drop 时同步封闭，所有委托编辑器操作在进入副作用阶段再次检查；项目停用、配置热替换同样生效。资源释放与异步回调保持来源归属。
- 独立 SDK 示例 0.9.0 通过公开打包入口生成消费者和两个提供者，宿主无示例插件 ID 或方法分支；`SERVICES.md` 随宿主 SDK 导出。

## 验证

```powershell
cargo fmt --check
cargo fmt --manifest-path plugins/capability-example/Cargo.toml --check
cargo check --workspace
cargo test --workspace --exclude editor-app
cargo test -p editor-app -- --test-threads=1
cargo build -p editor-app
./scripts/build-capability-example.ps1
cargo test -p plugin-runtime --test plugin_services -- --ignored --test-threads=1
cargo test -p plugin-runtime --test editor_requests -- --ignored --test-threads=1
cargo test -p editor-app native_service_provider_selection -- --ignored --test-threads=1
```

服务验收覆盖替换两个提供者、选择持久化、旧引用永久失效、启动顺序、必需/可选依赖、版本不兼容、
参数与结果类型、取消和超时、调用来源、禁止借用权限、循环调用、提供者陷阱、资源回收、
委托资源反复开关、异步授权延续，以及全局停用、项目停用和配置重建后的立即撤权。
GPUI 测试通过原生设置界面切换提供者，再由同一消费者得到不同独立包的返回值。

上述格式、编译、非 UI 工作区测试通过；编辑器普通测试 167 项通过。四项独立服务验收、
三项公共编辑器请求回归及一项原生提供者选择验收均通过。撤权修复后单独重跑提供者陷阱用例，
确认返回原始 OperationFailed 而不被被调用方退出覆盖；其余服务用例此前已在生命周期令牌版本通过。
规范轴与规格轴复审均通过。已有 Windows wasmtime/tree-sitter 链接警告及两处 dead_code 警告仍存在。

本工单不宣称已完成正式插件统一迁移或完整桌面人工验收；这些仍由后续迁移及 #21 工单完成。
