# 私有原生服务依赖

[English](../../en/sdk/dependencies.md)

除服务的 `process.service.<id>` 授权之外，还需协商 `dependencies: ^1` 并申请
`dependencies.prepare`。确认只允许对已声明或由 WASM 解析出的方案做**数据层面**的准备，
不允许执行安装程序或 Shell 脚本。

`process::Service.installation` 可选地提供 `dependencies::Plan`：

```json
{
  "program": "analysis-server",
  "args": ["--stdio"],
  "installation": {
    "executable": "server/bin/server.exe",
    "artifacts": [{
      "id": "server",
      "version": "1.2.3",
      "platform": "windows-x86_64",
      "sha256": "<64 hexadecimal digits of the exact archive>",
      "source": {"kind": "url", "url": "https://example.org/server-1.2.3.zip"},
      "format": {"kind": "zip"},
      "requires": []
    }]
  }
}
```

其他来源为 `{"kind":"package","path":"tools/server.zip"}` 与
`{"kind":"local","path":"C:/offline/server.zip"}`。包内路径是相对路径；本地来源路径必须是
绝对路径。裸可执行文件使用 `{"kind":"file","path":"bin/server.exe"}` 而不是 ZIP。校验和
作用于下载或来源文件，在解包之前校验。除字面回环 HTTP 地址外必须使用 HTTPS；回环 HTTP
用于支持本地仓库与测试夹具。URL 中带凭据会被拒绝。HTTPS 重定向仍须是 HTTPS；回环 HTTP
不重定向。请求使用 TLS 校验、有限超时与体积配额。

每个产物声明操作系统架构（例如 `windows-x86_64`），方案必须与当前宿主匹配。请使用候补服务
声明与钩子来选择平台相关的方案。`requires` 包含同一方案内的产物 ID；缺失引用、重复与循环
会在下载前失败。上限：32 个产物、64 KiB 方案、128 MiB 来源文件、256 MiB 解包后归档、
10,000 个 ZIP 条目。符号链接以及越界或重复的文件条目会被拒绝。

语言钩子可以返回 `language::Proposal.installation`。同一个有界、只读的钩子会在已准备的候选
实例上运行，此时尚未退役旧插件。它可以读取已确认的包、工作区与私有文件，但不能启动程序、
写文件、发布 UI 或再申请一次安装。宿主在其工具链边界内完成下载、哈希、解包与可执行文件
解析。

解析顺序：显式的用户或项目可执行设置；钩子给出的原生程序（需要 `process.exec`）；钩子安装；
声明式安装；声明的本地或 PATH 程序。显式但无效的路径直接失败，不回退。原生程序覆盖会跳过
随之不再使用的下载。钩子可以独立于依赖解析设置初始化与配置。

额外的运行时产物保留在私有版本目录中。以 `${dependency:runtime}/bin/entry` 开头的参数会解析
为该产物已校验的路径；普通参数保持字面值。请传 argv 数组，绝不用 Shell 模板。数据层面的
准备绝不修改全局 PATH，也不安装到系统包目录。

准备是显式安装或重装的一部分。启用与普通配置变更只读缓存；钩子方案发生变化且尚未缓存时，
会报告必须重试安装。匹配且已校验的缓存条目可离线复用。缓存键覆盖版本、平台、校验和、解包
格式以及任何安装程序定义，仅下载型缓存身份保持兼容。操作系统共享锁固定正在使用的服务与
进程文件。不可变回执会为保留的包版本累积已解析方案（含候补的工作区与配置方案），直到卸载
移除安装固定项。垃圾回收会跳过回执固定项与其他窗口的活动租约。

取消会停止准备并阻止候选版本发布；它不会回滚任意的原生副作用。等待其他准备锁的过程可取消。
HTTP 生产方从不写缓存文件，且并发上限为四个工作线程；被取消而停滞的网络读取会在 120 秒内
过期，且不阻塞调用方。安装进度区分下载或读取、SHA-256 校验、解包与插件安装。LSP 启动与
就绪由真实的协议客户端另行报告；已安装不等于已就绪。

HTTP 行为遵循
[ureq 配置契约](https://docs.rs/ureq/3.4.2/ureq/config/struct.ConfigBuilder.html)。

## 经授权的原生安装

产物可以附带 `installer`，但包必须另行声明并获得 `dependencies.install`。`dependencies.prepare`、
固定服务启动与 `process.exec` 都不授予这项权限。由 WASM 返回的方案与静态方案接受同样的检查。
更新新增权限需要重新确认；拒绝或准备失败都会保留先前版本可用。

```json
{
  "program": "setup.exe",
  "args": ["--output", "${target}", "--source", "${source}"],
  "target": "installed",
  "purpose": "Prepare the private analysis service",
  "kind": "service"
}
```

`program` 与 `target` 是相对该产物且不可越界的路径。原生程序来自其已校验的字节；不使用隐式
Shell 或环境中的程序查找。只有完整的 `${target}` 与 `${source}` 参数会被展开。外层方案中的
可执行文件指向产出的服务，例如 `server/installed/server.exe`。安装程序必须产出可重定位的
结果：完成后暂存目录会被重命名进不可变版本目录。

宿主展示真实的程序、argv、私有目标与用途，并在执行前等待一次性授权。编译器或大型项目 SDK
请把 `kind` 设为 `project_sdk`：在用户主动选择准备之前，不会读取或下载它们的来源。校验通过
后会有第二次确认，授权具体的原生执行。已完整存在的缓存条目无需重新执行。SDK 分类是包声明，
不是推断出的安全沙箱。

原生程序保留当前用户的文件与网络权限。私有目标**不是**操作系统沙箱。取消会终止受管进程并
清理暂存，但无法撤销程序已经造成的外部影响。Windows 上的执行会等待整个所属 Job 清空后才
清理或发布；Windows 是目前已验证的原生平台。

在需要具体授权时，无界面的 `Manager::install` 路径会按安全方向失败。交互式宿主创建
`InstallControl::with_installer_prompts`、轮询 `installer_prompt`，并且只在响应用户已显示的
选择时调用 `approve_installer(id, include_project_sdk)`。权限授予仍须经 `install_with_control`
完成；确认提示不能替代它们。过期或重复的批准会被拒绝，且 SDK 选择仅限该次请求。取消令牌
即拒绝或关闭提示。确认在 15 分钟后过期，原生执行有 10 分钟期限。重装是显式的重试操作。

安装程序是非交互的（stdin 关闭）；stdout 与 stderr 被排空且不做无界缓冲。非零退出、取消、
超时，或缺少或越界的服务输出，都会阻止完成标记与缓存发布。安装成功之后仍然使用常规的 LSP
就绪协议来报告语言服务就绪，而不是依据安装程序退出码。
