# Xray Pilot

Xray Pilot 是 Windows 上的 Xray 配置与运行管理工具。你可以导入原生 JSON、检查配置、预览修改、启动 Core，并通过托盘管理系统代理。

当前为 **0.1.1-rc.1 验证版（Pre-release）**，主要面向 Windows x64。支持 Global / Direct、配置历史和登录自启；PAC、TUN、Windows 服务部署及自动更新尚未提供。完整验收仍在进行，见 [已知限制](#已知限制)。

## 安装

### 1. 准备程序

从 [v0.1.1-rc.1 发布页](https://github.com/muidea/xray-pilot/releases/tag/v0.1.1-rc.1) 下载 [Windows x64 程序包](https://github.com/muidea/xray-pilot/releases/download/v0.1.1-rc.1/xray-pilot-0.1.1-rc.1-windows-x64.zip) 和 [SHA256SUMS](https://github.com/muidea/xray-pilot/releases/download/v0.1.1-rc.1/SHA256SUMS)，完整解压到固定位置，例如 `D:\Apps\xray-pilot`。这是供验证的预发布版；验证步骤见 [版本说明](docs/releases/v0.1.1-rc.1.md)。

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
Get-FileHash .\xray-pilot-0.1.1-rc.1-windows-x64.zip -Algorithm SHA256
```

### 2. 准备 Xray Core

从 [Xray 官方发布页](https://github.com/XTLS/Xray-core/releases) 获取适合 Windows x64 的 Core，解压并保留所需的 geo 数据文件。Xray Pilot 不捆绑或自动下载 Core。

也可以直接使用已有 Core，例如：

```text
C:\v2rayN\bin\Xray\xray.exe
```

本机已使用该路径下的 Xray v26.3.27 验证启停和本地代理。其他版本需重新校验配置。

### 3. 选择数据目录并启动

双击 `xray-manager.exe` 即可启动。默认数据目录为：

```text
%LOCALAPPDATA%\XrayManager
```

需要自定义目录时，从程序所在目录执行：

```powershell
.\xray-manager.exe --data-dir "D:\Xray Pilot Data"
```

后续启动和诊断都应使用同一数据目录。可以创建带上述参数的快捷方式。配置、Core 副本、历史和恢复记录保存在数据目录中，程序目录可以单独管理。

## 首次配置与连接

### 1. 设置 Core 和资源目录

打开 **Settings**：

- 在 **Official Xray Core executable** 选择 `xray.exe`。
- 在 **Resource directory** 填写证书、`geoip.dat`、`geosite.dat` 等文件的来源目录；使用上述 v2rayN Core 时可选 `C:\v2rayN\bin\Xray`。
- 点击 **Save settings**。

配置中引用的文件路径应相对于资源目录，例如 `certs/server.pem`。当前不接受越界路径或绝对文件依赖路径；遇到提示时需修改 JSON。应用会保存所需文件的副本，之后修改来源文件需重新校验、应用才会生效。

已保存的 Core 路径和自启选项会在重新打开界面后读取。0.1.1-rc.1 已修复此前已有 Profile 时设置显示为默认值的问题。

### 2. 创建或导入配置

打开 **Profiles / JSON**，输入配置名称，再选择 **Create** 或 **Import JSON…**。名称使用英文字母、数字、`-`、`_`，最多 80 个字符。

建议先导入 [examples/local-direct.json](examples/local-direct.json) 完成本机验证。它提供：

| 项目 | 默认值 |
| --- | --- |
| HTTP 代理 | `127.0.0.1:10809`，Tag 为 `http-in` |
| SOCKS 代理 | `127.0.0.1:10808`，Tag 为 `socks-in` |
| 出站 | 直接连接目标，无远端隧道 |

若 v2rayN 或其他程序正在使用这些端口，请先停止占用端口的实例，或修改 JSON 中的入站端口。同一个 Core 文件可以作为来源，但多个运行实例需要不同监听端口。

需要远端代理时，请导入你自己的原生 Xray JSON，填写真实的服务器、认证和 TLS 参数。[two-upstreams.json](examples/two-upstreams.json) 演示两个 SOCKS 上游按域名分流；其中 `20801`、`20802` 需要实际运行的上游服务。

### 3. 校验、预览并应用

1. 编辑 JSON，选择 **Proxy mode**。首次验证可先用 **Direct**。
2. 使用 **Global** 时，将 **HTTP inbound tag** 填为配置中的 HTTP 入站 Tag；示例为 `http-in`。
3. 点击 **Validate candidate / preview changes**，检查校验结果、依赖提示和修改差异。
4. 点击 **Apply validated configuration**。修改草稿或校验上下文后需要重新校验。
5. 确认顶部显示 Core 为 `Running`。校验失败时查看底部错误和 **Logs**，修正后重试。

两种模式的作用如下：

| 模式 | 系统代理与流量行为 |
| --- | --- |
| **Direct** | 恢复应用接管前的系统代理设置，Core 仍运行。手动连接 Core 的应用仍按 JSON 的出站和路由转发 |
| **Global** | 将当前用户的 Windows HTTP/HTTPS 系统代理指向选定的 HTTP 入站。仅使用该系统代理的应用受影响，Core 路由继续生效 |

Global 要求 HTTP 入站使用回环地址、固定数值端口且无认证。它不会接管全部设备流量；Core 显示 Running 也不代表远端代理已经可用。

### 4. 验证实际连接

在 PowerShell 中显式通过示例 HTTP 端口请求一个可访问的网站：

```powershell
curl.exe --proxy http://127.0.0.1:10809 --head https://example.com
```

按实际端口替换 `10809`。收到网站响应可用于检查这条代理连接；远端出口是否符合预期还需结合自己的上游与网络环境确认。界面中的 **Routing diagnostics** 只做静态预测，不能代替实际连接测试。

## 日常使用

| 操作 | 结果 |
| --- | --- |
| 关闭窗口 | Windows 下收起到托盘 |
| 托盘 **Open Xray Pilot** | 重新打开界面 |
| **Stop Core** | 停止 Core，并尝试恢复应用接管前的系统代理 |
| **Start / restart** | 启动或重启上次已应用的配置 |
| **Exit GUI (keep Core)** | 只退出界面，后台 Core 继续运行 |
| **Exit and restore proxy** | 停止后台、恢复代理并退出界面；如有恢复错误需继续处理 |

只退出界面后，再次打开程序会连接原后台。切换 Profile 后仍需校验并应用，选中名称本身不会切换正在运行的配置。

需要回滚时，在 **Committed snapshots / rollback** 中选择 **Preview this snapshot**，检查历史 JSON 和差异，再明确应用。

## 部署与开机使用

### 当前用户登录自启

在 **Settings** 中分别配置：

- **Start on Windows login**：登录后启动程序。
- **Connect on Supervisor startup**：后台启动后连接上次已应用的配置。

勾选后点击 **Save settings**。仅启用登录自启不会自动连接；自动连接需要已有成功应用的配置，且相关文件完整、依赖受支持。实际登录启动仍待完整验收，建议在自己的 Windows 环境中验证登录后的行为。

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

备份前先通过 **Exit and restore proxy** 完全退出，再复制整个数据目录。目录包含认证信息和恢复记录，请保存在受控位置。**Export JSON…** 只导出当前编辑器中的配置，不能代替完整备份。

更新时，完全退出后替换程序目录中的三个可执行文件，保持数据目录不变；更新前保留原程序与数据备份。跨版本数据迁移尚未完成验收。

移除时先取消登录自启并保存，再完全退出并确认代理已恢复，然后删除程序目录。需要清除历史和凭据时再删除数据目录；出现恢复错误时先处理错误并保留数据。

## 常见问题

| 现象 | 处理方法 |
| --- | --- |
| Core 路径失效或文件已移动 | 在 Settings 重新选择现有 `xray.exe`，保存后重新校验配置 |
| Core 启动失败或端口被占用 | 检查 **Logs** 和配置端口；排查 v2rayN 等其他实例，调整监听端口后重新校验 |
| 找不到 geo 文件或证书 | 检查资源来源目录及相对路径；补齐文件后重新校验 |
| Global 已启用，但部分程序未走代理 | 确认该程序使用 Windows 系统代理；也可手动填写 `127.0.0.1` 和实际 HTTP/SOCKS 端口 |
| 自动连接被拒绝 | 检查冻结文件是否完整及候选中的依赖提示；未覆盖的配置需显式校验、应用 |
| 界面连接后台失败 | 确认数据目录一致，尝试 **Reconnect Supervisor**，或用 CLI 查询 `status` |
| 操作报错，但不确定是否已生效 | 保留界面显示的 request ID，点击 **Query failed request**，或运行下方查询命令 |
| `PROXY_LEASE_CONFLICT` | 代理设置已被其他程序修改。应用会保留外部设置；先核查当前 Windows 代理及其他代理工具 |
| `RECOVERY_REQUIRED` / `PROXY_RECOVERY_REQUIRED` | 停止 Core、查询状态并保留整个数据目录；修复可信的原始文件后重启。不要删除租约或历史记录来绕过错误 |

```powershell
.\xrayctl.exe --data-dir "D:\Xray Pilot Data" logs
.\xrayctl.exe --data-dir "D:\Xray Pilot Data" operation REQUEST_ID
```

日志可能包含目标地址或认证信息，分享前请检查内容。

## 已知限制

当前仍缺真实 Windows 系统代理往返、实际登录启动、跨用户访问控制和完整 GUI 交互等验收证据。尚无历史容量限制和自动清理，使用中数据目录会持续增长；不应自行删除其中的恢复记录。

Linux 可用于 Direct 模式、诊断及开发验证；Windows 的系统代理、托盘和登录自启能力尚未移植。详细进度见：

- [当前遗留项](docs/phase0-2-backlog.md) 与 [C16 修复记录](docs/phase0-2-c16-closeout.md)
- [实现状态](docs/implementation-status.md)
- [源码构建、测试与高级诊断](docs/development.md)
- [架构与设计](docs/Xray%20Windows%20Manager架构与实现设计.md)
