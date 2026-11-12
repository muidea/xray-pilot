# Xray Pilot 交接文档

更新时间：2026-11-12

本文用于把当前工作区交给下一位开发者、评审者或 Windows 验证人员。当前已发布基线为 `efe1fcf`（`v0.1.1-rc.4`）；当前源码版本为 `v0.1.1-rc.18`；提交信息在交付前更新。不能把已发布的 `v0.1.1-rc.4` 当作当前源码版本。

## 当前产品边界

程序只服务当前登录用户会话。GUI、Supervisor、Xray Core、托盘和登录自启任务均使用当前用户身份；Windows Named Pipe ACL 只授予当前用户 SID，拒绝远端访问，不授予 SYSTEM 或服务账户访问。项目不注册 Windows 服务，也不提供 SYSTEM 级后台服务。

Windows Global / PAC 模式只接管当前用户的 WinINet 设置。Linux 主要用于 Direct 模式、诊断和开发验证。TUN、Helper、Windows 服务、安装器/自动更新和订阅管理不属于当前交付范围；PAC 基础链路已实现，完整系统验收见 V10。

## 已完成的源码收口

- 配置交互：协议模板、节点独立名称、多服务器/用户/账号编辑、原生字段编辑、未完成数值草稿持久化、缺项定位、草稿级撤销/重做和三种显示主题。
- PAC：内置模板支持全部代理、仅代理域名清单和清单直连；可直接添加/移除域名，并支持原生规则生成、导入/编辑、语法检查、有界 `FindProxyForURL` 执行测试、不可变回环 HTTP 发布、Windows PAC 租约目标、端口/版本切换、失败补偿和启动恢复；PAC 发布已纳入离线维护清理。
- 诊断与运维：静态顺序/遮蔽冲突提示、出站 TCP 延迟探测、Mixed 入站就绪、HTTP/Mixed 代理端点、WinHTTP 只读诊断、本地入口/PAC 服务状态分层、持久化日志（时间戳、代次上下文、凭据脱敏、轮转）、动态外部依赖 opt-in 与重校验。
- 配置校验：API/反向代理内部目标识别；常见节点参数提示；Reality/TLS 和路由字段提示；SOCKS/HTTP 账号 `user`/`pass` 缺项及错误类型提示。提示不回显凭据，不替代官方 Core 校验。
- 依赖与资源：常见 DNS、geo、ext、RAW 字段冻结；未分类字段会尝试启动和恢复，显式动态外部文件会跳过自动连接/恢复，不会静默猜测路径。
- 运行就绪：Direct/Global/PAC 对常见回环 HTTP、SOCKS5 入口执行有界握手和认证检查；不支持的协议、传输、非回环绑定等会明确报告未覆盖。
- 离线维护：`xrayctl maintenance` 支持默认预览、显式回收无引用孤儿资产和按需压缩终态操作；损坏的 `effect.json` / `settings.json` 证据会 fail-closed。
- Windows 当前用户安全：Named Pipe 双端 SID 核验、远端拒绝、Core Job 清理、安装目录旁 `data` 默认目录。
- GUI/托盘：顶栏只保留页面标题，主题在设置页选择，程序退出只通过托盘/窗口关闭入口；启动时按上次 Core 运行状态恢复，设置页可选择恢复或总是自动连接；确认/创建/重命名/未保存退出/应用预览统一对话框布局。

## 验证证据

Linux 当前源码已通过：

```bash
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
git diff --check
```

最近一次全工作区回归为 208 项通过，7 项需显式环境用例忽略（含官方 Core PAC 专项）；GUI 单元/无窗口回归 89 项通过。维护专项覆盖损坏终态证据阻断。

Windows GNU 原生验证使用用户指定的 `C:\v2rayN\bin\Xray\xray.exe`，已通过：

- Direct 应用、官方 Core 校验、Core Job 强杀清理、重启去重和系统代理保持不变。
- GUI 托盘 Open、隐藏/可见 Exit GUI、后台 Quit、GUI 重开和 Core 生命周期。
- GUI、独立 Supervisor、CLI、依赖脚本均使用安装目录旁 `data`，与工作目录无关。

可复测命令（在 Windows PowerShell 中执行）：

```powershell
.\scripts\windows-smoke.ps1 `
  -BinaryDir C:\path\to\xray-pilot `
  -CorePath C:\v2rayN\bin\Xray\xray.exe `
  -Gui

.\scripts\windows-portable-check.ps1 -BinaryDir C:\path\to\xray-pilot
```

验证真实 WinINet / PAC 前后可使用只读快照脚本（不会写注册表）：

```powershell
.\scripts\windows-proxy-snapshot.ps1 -Save .\proxy-before.json
# 执行受控测试
.\scripts\windows-proxy-snapshot.ps1 -Compare .\proxy-before.json
```

上述 smoke 使用隔离临时数据目录，不应修改用户已有 WinINet 设置或登录任务。当前环境中 `windows-integration` 的 ignored 登录任务测试曾被中断，不能把它当作真实注销/登录验收证据。

## 当前剩余验收项

| 编号 | 状态 | 交接说明 |
| --- | --- | --- |
| V01 | 待验收 | 真实 WinINet 四项设置往返、企业 PAC/外部代理竞争、连续切换和异常后的条件释放。 |
| V02 | 部分通过 | 仅验收当前用户登录自启；当前用户任务创建/查询/禁用已测，真实注销/登录入口行为仍需验证。 |
| V04 | 部分通过 | 需要完整进程强杀、每个发布/记账边界和真实断电矩阵。 |
| V05 | 部分通过 | 文件占用已测；磁盘满、安全软件拦截和断电恢复仍需真实 Windows 环境。 |
| V06 | 部分通过 | 当前用户 Named Pipe ACL 和异常帧已测；跨 SID 拒绝和更高压力仍需验证。SYSTEM/服务账户支持不属于范围。 |
| V07 | 部分通过 | Direct 异常恢复、慢启动和历史回滚已有证据；完整取消/失败组合及真实 Global 联测仍缺。 |
| V08 | 部分通过 | 核心托盘生命周期和单一退出确认已测；历史比较对话框、PAC 1–5 步、导入/导出、校验/差异/应用、断连和耗时操作仍需完整原生桌面验收。 |
| V09 | 部分通过 | 当前源码 GNU 构建和本机 smoke 已测；当前提交后的远端 CI、MSVC 构建、正式验证包和校验和仍需执行。 |
| V10 | 部分通过 | PAC 域名清单内置模板、1–5 步生成/检查/测试/应用、`AUTO_PROXY_URL` + `DIRECT`、发布/租约/端口切换/补偿/恢复已由 Linux fixture 12 项与官方 Core 专项覆盖；真实 Windows PAC URL 往返、客户端缓存、公网出口、完整 JScript 兼容、PAC 不可达回退和双向原生规则合并仍需验证。 |

能力项 F01–F05 仍按 [Phase 0–2 遗留清单](phase0-2-backlog.md) 保持“部分实现/部分完善”：F01 尚无历史 / 详情过期、自动维护和容量上限；F02 已补常见传输、DNS 和原生扩展字段及显式 Dynamic opt-in，但复杂组合仍有限；F03 已补 Mixed 入站和 HTTP/Mixed 端点，但特殊协议 / 传输、非回环、Proxy Protocol 等仍有限；F05 已补冲突分析、TCP 延迟、更多传输表单、草稿级撤销和 route context，但全部高级协议、语义级撤销和订阅能力仍保留。

2026-11-12 按 V1.2 设计逐项核对时登记的 N01–N08（WinHTTP 只读诊断、日志保留/脱敏、Dynamic 依赖、撤销、托盘状态、状态分层、路由上下文、导出提示）已在随后收口并加入正式回归；结论、影响与测试证据见 [设计文档核对](phase0-2-design-crosscheck.md)。该轮未复现新的 P1 缺陷；V/F 的真实系统验收状态未变。

## 重要文件入口

- [README.md](../README.md)：用户安装、配置和部署说明。
- [phase0-2-backlog.md](phase0-2-backlog.md)：验收编号和剩余能力清单。
- [phase0-2-remaining-closeout.md](phase0-2-remaining-closeout.md)：本轮收口内容与证据。
- [phase0-2-design-crosscheck.md](phase0-2-design-crosscheck.md)：按设计文档逐项核对后的剩余项、未登记设计项和证据边界。
- [code-review-2026-11-12.md](code-review-2026-11-12.md)：打包前代码评审发现与修复记录。
- [configuration-interaction.md](configuration-interaction.md)：配置交互实现合同。
- [implementation-status.md](implementation-status.md)：实现范围和历史验证记录。
- `crates/xray-config/src/security.rs`：当前用户 SID、目录保护和 Windows ACL。
- `crates/xray-supervisor/src/ipc.rs`：Windows Named Pipe、连接并发和双端身份检查。
- `crates/xray-supervisor/src/maintenance.rs`：离线维护和终态证据校验。
- `crates/xray-runtime/src/readiness.rs`：本地 HTTP/SOCKS 就绪握手。
- `crates/xray-manager-app/src/guided_config/`：配置模板和结构化编辑器。
- `crates/xray-pac/`：PAC 生成、校验与有界执行测试。
- `crates/xray-supervisor/src/pac_server.rs`：回环 PAC HTTP 发布与轮转。
- `crates/xray-runtime/src/lib.rs`：日志持久化/脱敏与本地就绪。

## 接手后的建议顺序

1. 保持当前用户模式不变，先运行 Linux 全工作区测试和 Clippy。
2. 在干净 Windows 测试账户中执行 smoke 与便携目录专项，确认当前用户 SID ACL 和 `data` 目录行为。
3. 单独安排真实 WinINet、注销/登录、跨 SID、磁盘满和断电测试；测试前保存代理设置及数据目录备份。
4. 当前源码已提交为 `ab08756`；提交后运行远端 CI，再生成正式验证包和 SHA-256 校验文件。
5. 只有在对应证据完成后，才更新 V01/V02/V04–V10 状态；不要用单元测试或 GNU 本机构建替代 MSVC、真实系统代理、PAC 客户端和人工交互验收。
6. 当前源码的 PAC、日志和动态依赖已提交为 `ab08756`，但尚未发布；发布前仍需完成 V09 的远端 CI、MSVC 和正式包验收。
