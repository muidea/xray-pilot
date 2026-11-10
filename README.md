# Xray Pilot

Xray Pilot 是 Windows 上的 Xray 配置与运行管理工具。你可以导入原生 JSON、检查配置、预览修改、启动 Core，并通过托盘管理系统代理。

当前为 **0.1.1-rc.4 验证版（Pre-release）**，主要面向 Windows x64。支持 Global / Direct、配置历史和登录自启；PAC、TUN、Windows 服务部署及自动更新尚未提供。完整验收仍在进行，见 [已知限制](#已知限制)。

## 界面版本说明

本版采用 **总览 / 配置 / 诊断 / 日志 / 设置** 五页界面，支持浅色、深色和跟随系统。托盘、任务栏及标题栏使用统一图标。

验证步骤见包内 `验证说明.md` 和 [验证指南](docs/validation-guide.md)。验证包统一使用 `xray-pilot-<版本>-windows-x64.zip`，以 `rc.N` 区分轮次。

## 安装

### 1. 准备程序

从 [v0.1.1-rc.4 发布页](https://github.com/muidea/xray-pilot/releases/tag/v0.1.1-rc.4) 下载 [Windows x64 程序包](https://github.com/muidea/xray-pilot/releases/download/v0.1.1-rc.4/xray-pilot-0.1.1-rc.4-windows-x64.zip) 和 [SHA256SUMS](https://github.com/muidea/xray-pilot/releases/download/v0.1.1-rc.4/SHA256SUMS)，完整解压到固定位置，例如 `D:\Apps\xray-pilot`。这是供验证的预发布版；验证步骤见 [版本说明](docs/releases/v0.1.1-rc.4.md)。

获取仓库源码后，也可按 [源码构建说明](docs/development.md#从源码构建) 自行构建。

解压后的 `xray-pilot` 目录包含：

| 文件 / 目录 | 用途 |
| --- | --- |
| `xray-manager.exe` | 日常使用的图形界面，双击启动 |
| `xray-supervisor.exe` | 独立后台运行程序，供命令行部署使用 |
| `xrayctl.exe` | 查询状态、停止和故障排查 |
| `VERSION.txt` | 版本和构建来源，反馈问题时可附上 |
| `examples/` | 可导入的示例配置 |
| `docs/`、`scripts/` | 详细说明及验证脚本 |

程序采用解压运行方式。在当前 Windows 用户下启动，并将程序和数据放在该用户可写的目录。

如需核对下载文件，在 PowerShell 中运行，并与随包提供的 `SHA256SUMS` 比较：

```powershell
Get-FileHash .\xray-pilot-0.1.1-rc.4-windows-x64.zip -Algorithm SHA256
```

### 2. 选择数据目录并启动

双击 `xray-manager.exe` 即可启动。Windows 默认数据目录为程序安装目录下的 `data`，例如 `D:\Apps\xray-pilot\data`；从快捷方式或其他工作目录启动也使用此位置。GUI、后台、CLI 和随包依赖安装脚本使用同一默认目录。需要自定义目录时，从程序所在目录执行：

```powershell
.\xray-manager.exe --data-dir "D:\Xray Pilot Data"
```

后续启动和诊断都应使用同一数据目录。可创建带上述参数的快捷方式。首次使用会进入设置向导，依次准备官方依赖、添加配置、检查并确认启动。

安装目录应可写；若目录受保护，请解压到可写位置或指定 `--data-dir`。旧版默认使用 `%LOCALAPPDATA%\XrayManager`，升级后如需继续使用旧数据，请显式指定旧目录，例如 `--data-dir "$env:LOCALAPPDATA\XrayManager"`；程序不会自动迁移或合并数据。启用过登录自启的安装仍绑定其原数据目录，请保持入口参数一致。

### 3. 自动准备官方依赖

在 **设置 → Core 与依赖** 保留推荐的官方版本 `v26.11.9`，点击 **下载 / 修复官方依赖**。程序会从 [XTLS/Xray-core 官方发布页](https://github.com/XTLS/Xray-core/releases) 对应版本下载 Windows x64 包及官方 `.dgst`，核对 SHA-256 后安装 `xray.exe`、`geoip.dat`、`geosite.dat`。

安装完成后自动填入 Core 路径与资源目录，点击 **保存设置**。程序包运行和依赖部署使用 Windows x64 自带的 PowerShell；无需安装 Python 或 Rust。

依赖保存在当前数据目录的 `dependencies/`：重复安装会校验并复用健康版本，损坏时可再次点击下载/修复；已验证的下载缓存可用于修复。新安装按版本分别保留，修复不会覆盖旧目录。下载失败不会切换已选依赖。

**检查已安装依赖** 可重新核对文件完整性；列表中的 **选用此版本** 会校验并选用已有版本。需要更换版本时，输入官方版本号后下载、保存设置，并重新校验和应用配置。安装和选择依赖不会自行重启或替换正在运行的 Core。

### 无界面部署与离线安装

也可先用随包脚本自动准备依赖，再打开 GUI 配置连接：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\windows-dependencies.ps1 -Action Install -DataDir "D:\Xray Pilot Data"
.\xray-manager.exe --data-dir "D:\Xray Pilot Data"
```

脚本输出已验证的 Core 和资源路径；GUI 在尚未配置 Core 时会读取已安装的版本，随后保存设置即可。指定其他官方版本时增加 `-CoreVersion v26.11.9`。

无法联网时，从 [官方 v26.11.9 发布页](https://github.com/XTLS/Xray-core/releases/tag/v26.11.9) 下载 `Xray-windows-64.zip` 和 `Xray-windows-64.zip.dgst`，放在同一目录，然后执行：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\windows-dependencies.ps1 -Action Install -DataDir "D:\Xray Pilot Data" -CoreVersion v26.11.9 -ArchivePath "D:\Downloads\Xray-windows-64.zip"
```

离线包同样需要通过 SHA-256、文件完整性和 Core 版本检查。已有自行从官方获取的 Core 也可在设置中手动选择其 `xray.exe` 及资源目录。

## 首次配置与连接

### 1. 设置 Core 和资源目录

打开 **设置**：

- 自动安装后，确认 **下次检查 / 应用使用的官方 Core** 和 **资源目录** 已填入受管版本路径。
- 手动准备 Core 时，选择从官方包解压的 `xray.exe`，资源目录填写 geo 文件及证书等文件所在目录。
- 点击 **保存设置**。

配置中引用的文件路径应相对于资源目录，例如 `certs/server.pem`。当前不接受越界路径或绝对文件依赖路径；遇到提示时需修改 JSON。应用会保存所需文件的副本，之后修改来源文件需重新校验、应用才会生效。

已保存的 Core 路径和自启选项会在重新打开界面后读取。

### 2. 创建或导入配置

打开 **配置**，选择 **新建** 或 **导入**，填写便于识别的显示名称（支持中文）。导入可粘贴 JSON，也可在 Windows 上选择 JSON 文件；程序自动分配内部标识，原生 JSON 不变。

JSON 编辑区提供行号、横向 / 纵向滚动和 **格式化 JSON**，宽高随父容器可用空间调整。配置页放大时显示更多内容，缩小时保留可用编辑面积并支持页面滚动；底部操作与编辑区分开。导入窗口内容较长时滚动中间表单，标题和底部操作按钮保持可见。格式化仅在主动点击时调整排版。

内容是可解析的 JSON 对象时，即使缺少协议、节点参数或路由引用，也可先 **保存待补充配置**；随后在 **配置 → 概览** 查看具体字段位置与补充提示，在 **原始 JSON** 中继续修改。JSON 语法错误需先修正，当前输入会保留。程序保留已有字段，不猜测服务器、认证参数，也不会把缺失的路由目标替换成直连。保存配置不会启动 Core；运行前仍须通过 **检查并预览**，由所选官方 Core 校验。

从文件读取时，默认沿用设置中的资源目录。只有相对文件依赖确实位于配置文件所在目录时，才勾选 **使用配置文件所在目录查找资源**；使用受管官方 geo 文件时通常保持默认。

建议先导入 [examples/local-direct.json](examples/local-direct.json) 完成本机验证。它提供：

| 项目 | 默认值 |
| --- | --- |
| HTTP 代理 | `127.0.0.1:10809`，Tag 为 `http-in` |
| SOCKS 代理 | `127.0.0.1:10808`，Tag 为 `socks-in` |
| 出站 | 直接连接目标，无远端隧道 |

若其他程序正在使用这些端口，请先停止占用端口的实例，或修改 JSON 中的入站端口。同一个 Core 文件可以作为来源，但多个运行实例需要不同监听端口。

需要远端代理时，请导入你自己的原生 Xray JSON，填写真实的服务器、认证和 TLS 参数。[two-upstreams.json](examples/two-upstreams.json) 演示两个 SOCKS 上游按域名分流；其中 `20801`、`20802` 需要实际运行的上游服务。

### 3. 校验、预览并应用

1. 在 **配置 → 概览** 选择 **应用后的系统代理方式**，再编辑 JSON。首次验证可先用 **不接管系统代理**。
2. 选择 **使用本机 HTTP 代理** 时，将 **用于接管代理的 HTTP 入站 Tag** 填为配置中的 HTTP 入站 Tag；示例为 `http-in`。
3. 点击 **检查并预览**，检查校验结果、依赖提示和修改差异。
4. 点击 **确认应用**（Core 未运行时为 **确认并启动**）。修改草稿、Core、资源目录或代理上下文后，需要重新检查；预览同时展示运行版本、待应用草稿与冻结资源。
5. 在 **总览** 确认显示 **Core 运行中**。校验失败时查看底部错误和 **日志**，修正后重试。

预览中的 **资源与自动恢复限制** 表示管理器尚未完整覆盖这些原生字段的自动恢复能力，不等于配置非法。出现此提示时，仍可在 Core 校验通过后手动确认应用，但自动连接、异常重试及自动恢复旧 Core 不可用。顶部显示中文摘要，逐项说明可展开并滚动查看。

两种方式的作用如下（CLI / 接口仍使用 Direct、Global）：

| 模式 | 系统代理与流量行为 |
| --- | --- |
| **不接管系统代理（Direct）** | 恢复应用接管前的系统代理设置，Core 仍运行。手动连接 Core 的应用仍按 JSON 的出站和路由转发 |
| **使用本机 HTTP 代理（Global）** | 将当前用户的 Windows HTTP/HTTPS 系统代理指向选定的 HTTP 入站。仅使用该系统代理的应用受影响，Core 路由继续生效 |

Global 要求 HTTP 入站使用回环地址、固定数值端口且无认证。它不会接管全部设备流量；Core 显示 Running 也不代表远端代理已经可用。

### 4. 验证实际连接

在 PowerShell 中显式通过示例 HTTP 端口请求一个可访问的网站：

```powershell
curl.exe --proxy http://127.0.0.1:10809 --head https://example.com
```

按实际端口替换 `10809`。收到网站响应可用于检查这条代理连接；远端出口是否符合预期还需结合自己的上游与网络环境确认。界面中的 **诊断** 只做静态预测，不能代替实际连接测试。

## 界面显示风格

在 **设置 → 外观** 选择 **跟随系统**、**浅色** 或 **深色**，分别跟随系统、使用浅色或使用深色。默认跟随系统，修改立即生效并自动保存，重开后保留选择。

## 日常使用

| 操作 | 结果 |
| --- | --- |
| 关闭窗口 | Windows 下收起到托盘 |
| 托盘 **打开 Xray Pilot** | 重新打开界面 |
| **停止 Core** | 停止 Core，并尝试恢复应用接管前的系统代理 |
| **启动 / 重启 Core** | 启动或重启上次已应用的配置 |
| **仅退出界面，后台继续运行** | 只退出界面，后台 Core 继续运行 |
| **退出程序（停止 Core 并恢复代理）** | 停止后台、恢复代理并退出界面；如有恢复错误需继续处理 |

只退出界面后，再次打开程序会连接原后台。切换配置后仍需检查并确认应用，选中名称本身不会切换运行版本。各配置的未应用草稿独立保存，重新打开界面可继续编辑；外部配置变化造成版本冲突时，先决定保留草稿内容或放弃草稿，再重新检查。上次已应用配置仍由运行记录引用，需先应用另一份配置后才能删除。

需要回滚时，在 **配置 → 历史** 中选择 **载入为草稿**，检查历史 JSON 和差异，再明确应用。

## 部署与开机使用

### 当前用户登录自启

在 **设置 → 启动行为** 中分别配置：

- **登录 Windows 后启动界面**：登录后启动程序。
- **后台启动后自动连接**：后台启动后连接上次已应用的配置。

勾选后点击 **保存设置**。仅启用登录自启不会自动连接；自动连接需要已有成功应用的配置，且相关文件完整、依赖受支持。实际登录启动仍待完整验收，建议在自己的 Windows 环境中验证登录后的行为。

先将程序放在长期使用的位置，再启用自启。需要移动程序时，先在原位置取消登录自启并保存，再完全退出，移动后重新启用。

### 仅运行后台

已有配置并希望通过终端管理时，可在 PowerShell 中执行：

```powershell
.\xray-supervisor.exe --data-dir "D:\Xray Pilot Data"
```

在另一个 PowerShell 窗口查询或控制：

```powershell
.\xrayctl.exe --data-dir "D:\Xray Pilot Data" status
.\xrayctl.exe --data-dir "D:\Xray Pilot Data" start
.\xrayctl.exe --data-dir "D:\Xray Pilot Data" stop
.\xrayctl.exe --data-dir "D:\Xray Pilot Data" shutdown
```

同一数据目录只允许一个后台实例；GUI 已启动后台时直接使用 `xrayctl.exe`，无需再启动第二个 Supervisor。首次创建、导入和应用配置建议使用 GUI。当前部署方式为当前用户进程，尚无 Windows 服务安装器。

### 备份、更新与移除

备份前先通过 **退出程序（停止 Core 并恢复代理）** 完全退出，再复制整个数据目录。目录包含认证信息和恢复记录，请保存在受控位置。**导出 JSON** 只导出当前编辑器中的配置，不能代替完整备份。

更新时，完全退出后替换程序目录中的三个可执行文件，保持数据目录不变；更新前保留原程序与数据备份。跨版本数据迁移尚未完成验收。

移除时先取消登录自启并保存，再完全退出并确认代理已恢复，然后删除程序目录。需要清除历史和凭据时再删除数据目录；出现恢复错误时先处理错误并保留数据。

## 常见问题

| 现象 | 处理方法 |
| --- | --- |
| 官方依赖下载失败 | 检查到 GitHub 的连接，重试或按上面的离线安装步骤准备官方包与 `.dgst` |
| 受管 Core 或 geo 文件损坏 | 点击 **检查已安装依赖**，输入对应官方版本后 **下载 / 修复官方依赖**，保存并重新校验配置 |
| Core 路径失效或文件已移动 | 下载/修复受管版本，或重新选择从官方包获取的 `xray.exe`，保存后重新校验配置 |
| Core 启动失败或端口被占用 | 检查 **日志** 和配置端口；排查其他代理实例，调整监听端口后重新校验 |
| 找不到 geo 文件或证书 | 检查资源来源目录及相对路径；补齐文件后重新校验 |
| Global 已启用，但部分程序未走代理 | 确认该程序使用 Windows 系统代理；也可手动填写 `127.0.0.1` 和实际 HTTP/SOCKS 端口 |
| 自动连接被拒绝 | 检查冻结文件是否完整及候选中的依赖提示；未覆盖的配置需显式校验、应用 |
| 界面连接后台失败 | 确认数据目录一致，尝试 **重新连接并读取状态**，或用 CLI 查询 `status` |
| 操作报错，但不确定是否已生效 | 保留界面显示的 request ID，点击 **查询原操作结果**，或运行下方查询命令 |
| `PROXY_LEASE_CONFLICT` | 代理设置已被其他程序修改。应用会保留外部设置；先核查当前 Windows 代理及其他代理工具 |
| `RECOVERY_REQUIRED` / `PROXY_RECOVERY_REQUIRED` | 停止 Core、查询状态并保留整个数据目录；修复可信的原始文件后重启。不要删除租约或历史记录来绕过错误 |

```powershell
.\xrayctl.exe --data-dir "D:\Xray Pilot Data" logs
.\xrayctl.exe --data-dir "D:\Xray Pilot Data" operation REQUEST_ID
```

底部状态栏显示消息摘要；长消息可点击 **完整消息** 滚动查看，并用 **复制完整消息** 保留全文。错误卡片中的详情和请求身份可独立展开查看。

修正配置后，成功应用、切换代理或启停 Core 会刷新旧的运行请求错误提示；尚未完成的恢复问题继续显示。历史失败记录仍可在日志和原请求查询中查看。

日志可能包含目标地址或认证信息，分享前请检查内容。

## 已知限制

自动依赖部署目前覆盖 Windows x64 的 Xray Core 与官方包内 geo 数据；用户自己的服务器配置、证书和密钥仍需自行准备。受管安装、缓存及冻结历史尚不自动清理。

当前仍缺真实 Windows 系统代理往返、实际登录启动、跨用户访问控制和完整 GUI 交互等验收证据。尚无历史容量限制和自动清理，使用中数据目录会持续增长；不应自行删除其中的恢复记录。

Linux 可用于 Direct 模式、诊断及开发验证；Windows 的系统代理、托盘和登录自启能力尚未移植。详细进度见：

- [当前遗留项](docs/phase0-2-backlog.md)
- [实现状态](docs/implementation-status.md)
- [源码构建、测试与高级诊断](docs/development.md)
- [架构与设计](docs/Xray%20Windows%20Manager架构与实现设计.md)
