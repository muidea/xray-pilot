# Phase 0–2 设计文档核对

日期：2026-11-12。依据 [V1.2 设计文档](Xray%20Windows%20Manager架构与实现设计.md)、[当前遗留清单](phase0-2-backlog.md)、[本轮能力收口](phase0-2-remaining-closeout.md) 与当前工作区源码。范围为 Phase 0–2 首版；PAC、TUN/Helper、安装器/自动更新和订阅不属于本轮交付。

## 结论摘要

**2026-11-12 收口更新：N01–N08 已实现并加入正式回归，PAC 自动代理链路已实现；C01–C20 与 F04 的既有回归继续通过。V01 / V02 / V04–V09、F01–F03 / F05 的真实系统验收边界保持不变，新增的 PAC 系统验收记为 V10 / 部分通过。**

下列设计项已在本轮收口；原登记缺口保留为历史核对记录，当前状态以本表“处理结果”与 [遗留清单](phase0-2-backlog.md) 为准：

| 编号 | 原未登记项 | 处理结果 |
| --- | --- | --- |
| N01 | WinHTTP 只读诊断未实现 | **已实现**：`Status.winhttp` 与诊断页只读展示；Windows 编译通过，未做真实 Windows 目视验收 |
| N02 | 日志保留、脱敏、时间戳与容量策略未实现 | **已实现**：`logs/supervisor.log` 持久化、UTC 时间戳、代次上下文、凭据掩码、4 MiB 轮转与 7 天备份清理 |
| N03 | 结构化表单的撤销/恢复未提供 | **已实现基础能力**：草稿级撤销/重做按钮与分组快照；复杂表单的语义级撤销仍按 F05 边界 |
| N04 | Dynamic 文件依赖策略未提供 | **已实现显式 opt-in**：绝对外部文件记录观察哈希并在应用/启动前重校验；自动连接/自动恢复禁用 |
| N05 | 托盘没有运行状态项或动态提示 | **已实现**：托盘顶部状态项显示 Core 与系统代理归属 |
| N06 | 状态枚举与健康分层未按设计完整暴露 | **已补充**：`listener_state`、`pac_state`、`winhttp` 与诊断页分层；未扩展为完整七态枚举 |
| N07 | 路由上下文与真实运行观测未覆盖 | **已补充上下文**：source IP/port、user、protocol、attrs 进入静态模拟；真实运行观测仍按确认的界面范围留作边界 |
| N08 | 导出时没有敏感内容提示 | **已实现**：导出前确认框提示认证信息风险 |
| N09 | 设计 §2.5 的 PAC 阶段口径 | **已按支持 PAC 的方向实现**；设计 §2.5 增加实现进展说明，V10 记录真实 Windows 验收缺口 |
| N10 | 设计 §12.1 事件订阅与轮询 | 保持 2026-11-10 已确认的轮询方案，不改为事件订阅 |

## 一、核对方法与本地复测

对当前工作区执行：

```bash
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
git diff --check
```

结果：

| 检查 | 结果 |
| --- | --- |
| 全工作区常规回归 | **208 通过**，7 项需显式环境的用例默认忽略 |
| GUI 单元 / 无窗口回归 | 89 通过，1 项官方模板专项默认忽略 |
| PAC / 配置 / Runtime | 12 / 20 / 9 通过 |
| Supervisor 库回归 | 43 通过，1 项官方 Core / 环境专项默认忽略 |
| 集成回归 | ipc_flow 2、runtime_flow 9、pac_flow 12、config_import 1、maintenance_cli 1 通过 |
| 格式、全目标严格 Clippy、diff 空白 | 通过 |

本环境没有官方 Core、真实 WinINet 和 Windows 桌面，因此 **Windows 原生 75 / Supervisor 23 / 官方 Core 8 项、GNU 冒烟和便携目录证据沿用 [收口记录](phase0-2-remaining-closeout.md) 的原验证范围**，本轮没有复跑，也没有据此新增系统验收结论。

## 二、Phase 0–2 验收矩阵逐行核对

状态含义：**通过** 表示当前源码有实现并有可复跑回归；**部分** 表示实现存在但目标系统证据仍缺，转入对应 V 项；**未实现** 表示设计能力不在当前代码中，转入对应 F/N 项；**受控** 表示只覆盖本地受控上游或模拟后端。

### Phase 0

| 验收场景 | 当前证据 | 结论 |
| --- | --- | --- |
| 原生 JSON 加载后由 Core 直接执行 | 冻结候选 `config.json` 直接作为 `run -config` 输入；`runtime_flow`、官方 Core 专项 | 通过 |
| JSON 不合法时拒绝启动 | `parse_document` / `parse_config` / `inspect_config` 阻断；GUI 显示字段位置 | 通过 |
| Core 启动失败显示错误 | `CORE_START_FAILED`、`last_error` 与界面错误卡片 | 通过 |
| 正确记录退出码 | `Process::try_wait` 与 `tick` 日志记录退出码 | 通过 |
| 不发生 Shell 命令注入 | Windows 使用 `CreateProcessW`，Linux 使用参数数组；未拼接 Shell 字符串 | 通过 |
| 显示 Core 实际运行状态 | `Status.core_state` 仅 Running / Degraded / Stopped | 部分（N06） |
| GUI 通过 Supervisor 控制 Core | GUI 只走 IPC，不持有 Core Job | 通过 |

### Phase 1

| 验收场景 | 当前证据 | 结论 |
| --- | --- | --- |
| A/B 双出站同时配置和使用 | `acceptance_edges`、官方 Core 受控 A/B | 通过（受控） |
| 指定域名访问 B，其他请求走 A | 同上，两个本地 SOCKS 上游 | 通过（受控） |
| B 故障不自动切换到 A | B 拒绝或停止监听时 A 记录不增加 | 通过（受控） |
| 修改配置失败恢复旧配置 | C01 / C09 修复；`failed_apply_restores_actual_running_or_stopped_state`、`failed_candidate_rolls_back_process_formal_json_and_proxy` | 通过 |
| 成功应用后 JSON、冻结副本、Revision、清单哈希一致 | `connect_on_start_and_history_roundtrip`、候选/历史发布恢复回归 | 通过 |
| 来源证书 / geo 更新后 Frozen 代次不变 | `resources` 冻结与校验回归 | 通过 |
| 冻结副本缺失或哈希错误报 `RESOURCE_INTEGRITY_FAILED` | `verify` 与 `missing_frozen_resource_blocks_auto_connect` | 通过 |
| Dynamic 文件变化后重新校验并通过新事务应用 | 显式 Dynamic opt-in 记录观察哈希，应用/启动/重启前重校验，变化返回 `RESOURCE_REVALIDATION_REQUIRED` | 通过（受限，自动恢复禁用） |
| 原生扩展依赖无法完整枚举时标注 CoverageIncomplete | `coverage::uncovered` 白名单、候选 `resources.notes` 与预览 | 通过 |
| 绝对路径无法绑定冻结内容时展示修改 | 默认返回 `RESOURCE_PATH_REQUIRES_EDIT`；显式 opt-in 时按 Dynamic 记录并重校验 | 通过 |
| 首次启动无旧代次、应用失败恢复 Stopped/Unmanaged | 补偿回归读取实际前态，不伪造 PreviousActive | 通过 |
| 正式资产部分替换后退出按提升凭据恢复 | `precommit_promoted_asset_is_conditionally_restored_on_startup` | 通过（模拟） |
| 清单原子替换前 / 后退出 | 提交点前后恢复与提交凭据回归 | 通过（模拟） |
| 补偿失败或恢复资产缺失进入 RecoveryRequired | `unresolved_asset_recovery_remains_pending_until_repair` | 通过 |
| Applying 候选 Core 异常退出由所属事务补偿 | `global_waits_for_slow_listener...` 与失败应用回归 | 通过 |
| 计划停止的旧 Core 退出事件不进入故障重试 | `stale_retry_cannot_modify_new_generation_or_lease`、停止路径 | 通过 |
| 未知 JSON 字段不丢失 | `Store` 与原生编辑器保留未知字段回归 | 通过 |
| URL 缺入站 / 网络上下文返回 Unknown | `routing::simulate` 测试 | 通过 |
| 前序原生规则不受支持时不跳过 | `unknown_earlier_rule_blocks_default_prediction` | 通过 |
| 静态模拟只显示预期出站，实际出口 Unverified | `RouteResult.actual_outbound = "Unverified"` 与诊断页 | 通过 |

### Phase 2

| 验收场景 | 当前证据 | 结论 |
| --- | --- | --- |
| Windows Global 正确启用 | FakeProxy 回归；真实 WinINet 仍缺 | 部分（V01） |
| Global 下 HTTP 入站端口变化联动 | 每次应用从候选重新解析 `http_endpoint`；未做真实 WinINet 联测 | 部分（V01 / V07） |
| 缺失 HTTP 入站 / 端点不兼容时拒绝接管 | `http_endpoint` 校验协议、回环、数值端口和无认证 | 通过 |
| 恢复原有 PAC | 租约根快照包含 PAC / flags 并条件恢复；真实企业 PAC 未测 | 部分（V01） |
| 持有租约期间切换 Profile，leaseId 和根快照不变 | `transition` 保留根快照；租约回归 | 通过（模拟） |
| Windows 写入成功、租约记账前退出 | `startup_recovers_write_before_finalize` | 通过（模拟） |
| IPC 超时后同一 requestId 返回原结果 | `ipc_flow`、操作记录回归 | 通过 |
| 清单已提交、回包前退出且关闭自动连接 | 提交结果重建与条件释放回归 | 通过（模拟） |
| 旧代次仍持有租约、关闭自动连接后启动 | `committed_request_survives_restart_and_stale_proxy_is_released` | 通过（模拟） |
| 未提交事务恢复时关闭自动连接 | 启动恢复回归不短暂启动旧 Core | 通过（模拟） |
| 自动连接失败 / 冻结资源不可用条件释放代理 | `missing_frozen_resource_blocks_auto_connect`、恢复回归 | 通过（模拟） |
| 启动时第三方修改代理保留外部设置 | `stop_works_with_corrupted_snapshots_and_external_proxy_conflict` | 通过（Fake） |
| Released 后回放历史 finalize 不复活租约 | `finalize` 与租约回归 | 通过 |
| g1 退避中提交 g2 后丢弃过期任务 | `stale_retry_cannot_modify_new_generation_or_lease` | 通过 |
| 同一代次重启后旧实例退出事件被丢弃 | `runtimeInstanceId` / `recoveryEpoch` 检查 | 通过 |
| 新 Supervisor 不执行旧 epoch 事件 | `supervisorEpoch` 与新启动恢复流程 | 通过 |
| 提交 g2 后响应丢失、旧 expectedGeneration 重试 | 去重先于版本检查的回归 | 通过 |
| GUI PID 变化后仍能查询同一 requestId | `callerKey` = 当前用户 SID + 持久 installation ID；IPC 回归 | 通过 |
| 同一 requestId 不同 payload 返回 `REQUEST_ID_CONFLICT` | `changed_request_content_is_rejected_without_blocking_the_store` | 通过 |
| Accepted 后、执行意图前退出 | `preparation_and_accepted_requests_have_distinct_recovery_outcomes` | 通过 |
| 原请求提交成功但终态记账失败 | 从清单重建结果；结算失败阻断后续提交 | 通过 |
| 终态详情按保留策略压缩并返回 ResultDetailsExpired | 当前 `--compress-operations` 是无损 envelope，不清理详情 | 未实现（F01） |
| 操作记录损坏或索引矛盾 | 统一 `OPERATION_RECOVERY_REQUIRED` 回归 | 通过 |
| GUI 崩溃后 Supervisor 继续运行 | 进程分离设计；自动化未见独立进程强杀 GUI 回归 | 部分（V08） |
| Core 崩溃后检测与处理 | 重启预算与故障恢复回归；真实 Core 专项见 V07 | 通过（模拟） |
| 外部代理修改不盲目覆盖 | `PROXY_LEASE_CONFLICT` 与外部修改回归 | 通过（Fake） |

## 三、剩余 V 项核对

| ID | 设计依据 | 当前源码状态 | 仍缺证据 | 结论 |
| --- | --- | --- | --- | --- |
| V01 | §7、§8、§20 | WinINet 四项选项读写、根快照、读回、条件恢复和外部冲突均已实现；WinHTTP 只读诊断已补；系统代理测试使用 FakeProxy | 真实四项设置往返、企业 PAC、连续切换、异常释放 | 保持待验收 |
| V02 | §13.4 | Task Scheduler 当前用户任务、所有权、外部修改和幂等禁用已实现 | 实际注销 / 登录入口、权限拒绝 | 保持部分通过 |
| V03 | §6、§20 | 受控 A/B、禁止回退已通过 | 不代表公网账号 / TLS / 远端出口 | 保持已关闭（受控上游） |
| V04 | §5.4.2、§5.4.3 | C01–C13 的持久化边界均有恢复回归；非运行写操作有提交点前后注入 | 每个发布 / 记账边界的系统化进程强杀矩阵、真实断电 | 保持部分通过 |
| V05 | §5.4.2、§14.1 | 真实 Windows 文件句柄占用下回滚通过 | 磁盘满、安全软件、断电恢复 | 保持部分通过 |
| V06 | §12.1、§14 | 管道 SID ACL、超长 / 截断 / 慢读、并发上限和退出检测 | 跨 SID 拒绝、更高压力；SYSTEM / 服务账户不在范围 | 保持部分通过 |
| V07 | §11.6 | Direct 异常退出重启、慢启动、历史回滚、混合失败预算 | 完整失败组合与真实 Global 联测 | 保持部分通过 |
| V08 | §13 | 托盘单一退出确认、打开/启停、配置列表、托盘状态、隐藏窗口唤醒、草稿和设置状态机、历史比较对话框、PAC 1–5 步 | 导入 / 导出、校验 / 差异 / 应用、历史比较/载入、断连、耗时操作的真实桌面交互 | 保持部分通过 |
| V09 | §19、§20 | 当前源码 GNU 交叉构建、GUI / Supervisor / 官方 Core 隔离冒烟、便携目录 | 当前提交的远端 CI、MSVC、正式验证包和校验和 | 保持部分通过 |
| V10 | §9、§20 P3 | PAC 域名清单内置模板（全部代理/仅代理清单/清单直连）、1–5 步生成/检查/测试/应用、不可变回环发布、`AUTO_PROXY_URL` + `DIRECT`、租约与代次联动、端口切换、补偿、启动恢复；Supervisor PAC 12 项与官方 Core 1 项（按环境忽略） | 真实 Windows PAC URL 往返与客户端缓存差异、公网出口、完整 JScript 兼容、PAC 不可达回退 | 部分通过 |

## 四、剩余 F 项核对

| ID | 设计依据 | 已实现 | 仍保留的边界 | 结论 |
| --- | --- | --- | --- | --- |
| F01 | §5.6.3、§12.5.3 | 离线预览、无引用孤儿回收、无损终态压缩、终态证据 fail-closed | 历史 / 详情过期、自动维护、缓存 / 依赖版本清理、容量上限；没有 `ResultDetailsExpired` | 状态准确 |
| F02 | §5.6 | 常见路由、DNS、TLS 证书和 geo / ext 冻结；已扩展常见传输、DNS、sockopt 和原生扩展字段；绝对外部文件可显式 Dynamic opt-in，记录观察哈希并在应用/启动前重校验 | 复杂协议组合和未知扩展仍可能降低覆盖；Dynamic 不承诺旧内容回滚 | 状态准确，需补边界说明 |
| F03 | §11.5 | Direct / Global / PAC 均等待回环 HTTP / SOCKS5 / Mixed 握手与认证，有期限、取消和提前退出监测；状态提供 listener / PAC 分层 | 无入站、特殊协议 / 传输、非回环、Proxy Protocol、非数值端口；完整 Core 七态枚举仍待细化 | 状态准确 |
| F04 | §12.1 | `-test` 原始诊断按 16 KiB 分块进入有界会话日志，错误只留摘要 | — | 已完善 |
| F05 | §5.2、§13.2 | 协议模板、节点名称、多服务器 / 用户 / 账号、原生字段编辑、数值草稿、缺项定位、托盘配置列表、托盘状态、草稿级撤销/重做、路由上下文、冲突提示、TCP 延迟探测、更多传输表单 | 全部高级协议 / 组合规则、语义级撤销、订阅 / 分享链接 | 状态准确，需补边界说明 |

## 五、未进入遗留清单的设计要求

### N01–N08 收口实现

- **N01 WinHTTP**：`Status.winhttp` 返回只读的 WinHTTP 默认会话访问类型、代理和 bypass；诊断页展示其用途（独立于 WinINet 的 WinHTTP 客户端），Windows GNU 全目标编译通过。机器级 WinHTTP 从不写入。
- **N02 日志**：Supervisor 写入 `logs/supervisor.log`，每行带 UTC 时间戳、操作/代次上下文；`password`/`pass`/`id`/`uuid`/`Authorization` 等字段脱敏；4 MiB 轮转一个备份，7 天后清理旧备份；`GetLogs` 继续返回内存中的有界会话日志。
- **N03 撤销/重做**：草稿级撤销栈按 600 ms 分组，界面提供 **撤销编辑 / 重做**；原始 JSON 文本框仍保留 egui 自带编辑撤销。复杂语义冲突回退仍归 F05。
- **N04 Dynamic**：显式 opt-in 后允许绝对外部文件，记录来源、观察哈希和时间；应用/启动/重启前重新校验，变化返回 `RESOURCE_REVALIDATION_REQUIRED`；`complete=false`，自动连接与自动恢复禁用。
- **N05 托盘状态**：托盘顶部只读状态项显示 Core 状态与系统代理归属，并随状态刷新。
- **N06 状态分层**：`Status` 增加 `listener_state`、`pac_state`、`winhttp`；诊断页分别展示本地入口、PAC 服务与 WinHTTP。完整设计七态枚举未全部暴露，保留为后续界面细化。
- **N07 路由上下文**：`RouteContext` 增加 source IP/port、user、protocol、attrs，静态模拟按原生顺序匹配；缺失上下文仍返回 Unknown。真实运行观测仍按确认的界面范围只显示“未验证”。
- **N08 导出提示**：导出 JSON 前显示包含地址、UUID、密码、私钥等敏感信息的确认框。

以上均有正式回归；具体命令与计数见第六节。

## 五、本轮 PAC 收口

本轮已实现 PAC 基础链路：

- `xray-pac`：从原生域名规则保守生成 PAC；语法校验；有界 `FindProxyForURL` 执行测试；`dnsResolve` / `isInNet` 等网络辅助函数明确返回 Unsupported。
- Supervisor：PAC 模式纳入运行事务与租约；不可变 `runtime/pac/<sha256>/proxy.pac` 发布；仅回环 HTTP 服务；端口冲突 fail-closed；端口/版本切换保留旧监听直至提交；失败补偿恢复旧 PAC；启动恢复与自动连接。
- Windows：PAC 目标快照使用 `AUTO_PROXY_URL` + `DIRECT`（允许脚本 DIRECT） 与不可变 URL，释放时恢复根快照；真实 WinINet 往返仍归 V01 / V10。
- 诊断与入站：静态冲突提示、Mixed 入站、HTTP/Mixed 代理端点、出站 TCP 延迟和更多传输表单。
- GUI：PAC 页 1–5 步（范围、域名、生成检查、测试、应用）；域名清单添加/移除立即保存并切回模板；生成/导入/编辑/测试/端口在步骤或高级设置；当前 PAC URL 与草稿哈希不一致会提示；配置页 PAC 模式、总览/托盘切换、历史 PAC Revision 展示。
- 回归：PAC crate 12 项、Supervisor PAC 流程 12 项、官方 Core PAC 1 项（按环境忽略）、Dynamic 2 项、日志 2 项、路由上下文/冲突 4 项、编辑撤销 1 项、Mixed 1 项、延迟 1 项；全工作区 **208 项通过**，7 项需显式环境忽略。

仍未覆盖：真实 Windows PAC URL 往返、浏览器/客户端缓存差异、公网出口、PAC 自定义脚本的全量 JScript 兼容和内存级隔离。V10 保持部分通过。

## 六、核对边界与未发现项

- 本轮没有新增确定性复现探针，因此不新增 C 编号。
- 真实 WinINet、注销 / 登录、跨 SID、磁盘满、安全软件拦截、断电、MSVC CI 和完整桌面交互仍不在本机能力范围；V01 / V02 / V04–V09 不能因本轮常规测试而关闭。
- Linux 本机已通过 208 项常规回归和严格静态检查；Windows GNU 全目标检查通过；这不替代 Windows 原生、真实 WinINet 与官方 Core 证据。
- 设计 §20 中 P3–P5 的 PAC、安装 / 卸载、Core 更新、TUN / Helper 验收行仍不在 Phase 0–2 范围。

## 七、建议顺序

1. 保持 V01 / V02 / V04–V09 现有状态，按 [交接文档](handoff.md) 的环境清单继续补真实系统证据。
2. 在 F02 / F05 / V08 中继续登记剩余组合与语义级边界，避免“已实现”被理解为覆盖全部设计能力。
3. N01 / N02 已按当前源码实现并纳入交付说明；后续可把日志级别、保留策略和 WinHTTP 诊断扩展为独立能力项。
4. 设计 §2.5 的 PAC 阶段口径已按“当前源码支持基础 PAC、完整验收归 V10”更新说明。
5. 当前源码已提交为 `367f96c`（rc.17）；下一步跑远端 CI、生成正式验证包，并按 V01–V10 补齐系统验收。
