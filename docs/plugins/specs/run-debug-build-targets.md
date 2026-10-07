# 通用目标发现与准备接缝（#55）

本次补全采用 `run.targets 1.0`：`discover` 返回带类型版本、稳定身份、相对来源和可移植 binding 的候选；`prepare` 只构建并返回真实可执行产物，不执行最终程序。每次调用固定原提供者实例、工作区、配置快照和独立资源根，停止、超时或撤销释放该根，迟到结果不能启动。手动程序与 Shell 配置保持原有语义。

发现配置保存为通用 provided 目标；它的构建列表含一个提供者准备动作。Build 仅运行该动作。Run/Debug 运行一次准备动作，产物仅填入本次不可变计划，再完成启动前步骤，最后交给执行或调试提供者。不能从允许丢片段的输出历史解析产物，不能重复执行最终程序。

Rust WASM 用 [Cargo metadata](https://doc.rust-lang.org/cargo/commands/cargo-metadata.html) 的 workspace_members 和 bin targets 发现虚拟工作区、显式及隐式多 bin；每个目标提供 Debug/Release 候选。稳定身份包含相对 manifest、bin 和模式，不存 Cargo 的绝对 package ID。构建选定 manifest/bin/profile，实时读取受控 stdio，按 [compiler-artifact 消息](https://doc.rust-lang.org/cargo/reference/external-tools.html#json-messages) 解析唯一 executable；成功退出且找到匹配产物才成功，支持自定义 target directory，不推算路径、不用 cargo run，不安装编译器。

重发现仅提供候选与修复建议，不改配置；目标消失阻止新启动，撤销贡献保留保存配置。确认选择新候选可以修复旧 binding，同时保留名称、参数、环境和其他步骤。配置格式提升为 2，读取原有版本 1；旧 Cargo 配置需显式修复，不能静默改成新产物。
