# Capability Example

开发验证插件，演示独立能力版本、类型化文件访问和原生文本界面。基础 API、package.assets、ui.native、workspace.files、storage.private 分别协商 1.x；不存在的可选接口会降级显示。0.2.0 默认每个工作区独立实例。

安装时需要批准 assets.read（读取包资源）、workspace.read（读取所属工作区）与 storage（读写实例私有文件）。不申请进程、网络或剪贴板权限。菜单命令“检查类型化错误”验证未知操作、错误参数与路径越界的明确返回。

scope-write / scope-read 将工作区的 source.txt 与私有 value.txt 一起显示；scope-probe 接收公开 Operation JSON，并将 SDK 的类型化结果显示为文本。诊断命令验证跨实例句柄拒绝、应用级实例不具有工作区权限，以及显式释放后的句柄失效。

SDK 提供 open_workspace、open_data、read_file、write_file、close_resource；句柄由宿主签发，不应持久化。workspace.files 1.0 只读，storage.private 1.0 支持私有根目录直接子文件的原子写入，每文件最多 1 MiB，累计受清单 storage_limit 约束。用户设置、其他工作区数据及宿主快照均不在可读根目录内。

通过宿主公开的 --plugin-cargo 入口构建，不使用宿主业务源码路径。开发打包脚本为 build-capability-example.ps1；输出仅用于新平台迁移验证，不纳入正式发行包。
