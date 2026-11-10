# 首次部署、官方依赖与界面偏好

适用版本：v0.1.1-rc.2，Windows x64。用户安装步骤见 [README](../README.md)。正式部署从 [XTLS/Xray-core 官方发布页](https://github.com/XTLS/Xray-core/releases) 获取依赖，默认使用已验证版本 v26.11.9。

## 使用入口

首次没有 Core 设置时 GUI 打开 Settings，用户点击 Download / repair official Core 后自动下载、校验并安装。安装完成填入 Core 和资源目录，随后明确 Save settings；配置的校验、预览和应用流程保持不变。

脚本入口为 [windows-dependencies.ps1](../scripts/windows-dependencies.ps1)。它仅管理依赖，不修改 manager.json、登录任务、系统代理、运行代次或正在运行的 Core。GUI 仍通过 Supervisor 的 SaveSettings 保存设置。依赖工作在独立线程/PowerShell 子进程中执行，不阻塞 GUI 或运行监督协调器。

支持动作：

| 动作 | 行为 |
| --- | --- |
| Install | 安装指定官方版本；已有健康副本则复用，损坏则发布新安装目录 |
| Status | 核对受管文件哈希，返回当前可用版本和各安装的完整性结果 |
| Select | 重新校验指定安装，再更新受管默认选择；不会直接切换运行 Core |

参数 CoreVersion 为官方版本 Tag，例如 v26.11.9。ArchivePath 指向离线官方 zip，旁边须有同名 `.zip.dgst`。不接受自定义下载主机或任意解压目录。

## 校验、发布与恢复

网络来源固定为 `https://github.com/XTLS/Xray-core/releases/download/<version>/Xray-windows-64.zip` 及对应 `.dgst`。下载使用 TLS、有限超时与重试。先核对官方 SHA2-256，再仅提取明确命名的 xray.exe、geoip.dat、geosite.dat；拒绝缺失、重复、空或过大的条目，不展开任意 ZIP 路径。离线包也执行相同检查，并执行 `xray.exe version` 核对目标版本。

每个完整安装保存版本、安装 ID、官方来源、归档 SHA-256 和逐文件 SHA-256 的 manifest。完整性检查会重新读取这三个文件；归档缓存每次复用也须重新核对。官方摘要用于检查下载完整性，不替代本机目录访问控制，也不宣称提供上游未提供的数字签名。

目录位于数据目录下：

```text
dependencies/
  install.lock
  current.json
  downloads/<version>/Xray-windows-64.zip[.dgst]
  xray/<version>/<installation-id>/
    manifest.json
    xray.exe
    geoip.dat
    geosite.dat
```

安装器使用独立排他文件锁；竞争安装明确失败，用户可稍后重试。临时目录完成校验后通过同卷目录移动发布，current.json 使用原子替换。PowerShell 5.1 的 File.Replace 调用使用 NullString 保留真正的空备份参数。当前选择发布失败保留旧文件；损坏安装保留供核查，修复使用新 ID，不原地覆盖。中断留下的临时目录和缓存不被当作健康安装。

归档缓存可重新下载；受管依赖是校验输入来源。Supervisor 在验证配置时仍复制 Core/资源到现有冻结目录，运行代次继续固定对应哈希。下载、选择或修复受管版本不会改变冻结文件；用户需要重新校验并应用才能切换运行版本。旧安装、下载缓存和冻结历史尚无自动回收。

GUI 下载结果绑定设置上下文、资源目录及请求版本。下载期间用户修改 Core/资源/自启选项、保存设置或更改目标版本后，安装结果仍保留，但不会覆盖当前选择。初始 Status 在没有 Core 设置时才自动选入健康受管版本；已有手动路径保持不变。

## 显示风格

Settings 的 Appearance 提供 Follow system、Light、Dark。默认跟随系统，切换立即应用并原子保存到数据目录的 config/appearance.json；重开时恢复。外观偏好由 GUI 管理，独立于 Core 设置和运行事务。偏好损坏时保留原文件、提示读取错误并使用默认主题，用户明确选择后才保存新的值。

## 验证

正式 GUI 状态回归覆盖异步安装结果、用户修改/保存保护、失败和工作线程断连；主题回归覆盖三种选择的实际 egui 偏好、持久化和损坏文件保留。Windows 另有内嵌 PowerShell 的中文/空格路径 JSON 协议回归。

[windows-dependencies-check.ps1](../scripts/windows-dependencies-check.ps1) 在独立临时目录验证官方安装、健康复用、geo 损坏检测、新目录修复、原子选择失败、并发锁、离线包、错误版本和错误 SHA-256 拒绝，以及 manager.json 未修改。默认执行实际官方下载；ArchivePath 可指定已从官方获取并保留摘要的缓存。

本能力不下载用户服务器配置、证书或密钥，不提供系统安装器、应用程序自动更新、PAC/TUN 依赖或历史 GC。其他验收边界继续以 [遗留清单](phase0-2-backlog.md) 为准。

本轮使用 Windows 自带 PowerShell 完成 v26.11.9 实际官方下载；归档 SHA-256 为 `1262522266337b824f07beb8e1375786b437683b3890105469088f8e17e0a69d`。上述隔离安装/修复脚本验收通过。Windows 原生 GUI 16 项回归通过，包含内嵌脚本对规范化后的中文/空格路径处理。
