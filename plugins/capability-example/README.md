# Capability Example

开发验证插件，演示独立能力版本、类型化包资源读取和原生文本界面。基础 API、package.assets、ui.native 分别协商 1.x；不存在的可选接口会降级显示。

安装时需要批准 assets.read（读取本插件包中的资源）。插件不申请工作区、进程、网络或剪贴板权限。菜单命令“检查类型化错误”验证未知操作、错误参数与路径越界的明确返回。

通过宿主公开的 --plugin-cargo 入口构建，不使用宿主业务源码路径。开发打包脚本为 build-capability-example.ps1；输出仅用于新平台迁移验证，不纳入正式发行包。
