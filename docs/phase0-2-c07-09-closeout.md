# C07–C09 代码收口记录

日期：2026-11-10。范围：上一轮 [遗留项复核](phase0-2-remaining-review.md) 中 C07/C08/C09；保留 Phase 0–2 首版边界。

**C07–C09 已修复并通过正式回归。当前已确认的 C01–C09 全部关闭；其余系统验收、保留/压缩与界面能力仍按 [遗留清单](phase0-2-backlog.md) 跟踪。**

收口后的再次核对发现 C10–C12，随后已修复；见 [当前复核报告](phase0-2-current-review.md) 与 [最新收口记录](phase0-2-c10-12-closeout.md)。本记录描述 C07–C09 的修复证据，不表示后续核对没有新增问题。

## 1. C07：非运行写命令的执行与结果证据

实现：[effects.rs](../crates/xray-supervisor/src/effects.rs)、[Supervisor](../crates/xray-supervisor/src/lib.rs)。

CreateProfile、DeleteProfile、ValidateDraft 和 StageHistory 在准备工作之前保存 `runtime/transactions/<transaction_id>/effect.json`，绑定原请求摘要与事务身份。Core/资源/JSON 的准备只产生私有副本；正式 Profile 或候选 `candidate.json` 的最终公开发布才是本操作的提交点。

发布前记录：目标路径、前态存在性/哈希、目标内容或删除目标、成功结果。发布后核对实际文件端点，并持久保存 outcome，再交给 Operation 保存响应。任何无法判断或无法持久结算的情形都会阻断后续普通写入。删除采用文件删除，Unix 补刷父目录；Windows 删除的断电持久性仍属于 V05 未完成的系统验收范围。

| 恢复证据 | 处理 |
| --- | --- |
| 只登记 Accepted，没有执行意图 | INTERRUPTED_BEFORE_APPLY，不自动执行 |
| 有准备意图，无公开发布 | INTERRUPTED_COMPENSATED；私有准备副本继续按现有保留策略保存 |
| 发布意图存在，目标仍等于前态 | INTERRUPTED_COMPENSATED，不自动发布 |
| 发布意图存在，目标等于后态 | 重建原成功结果并结算 |
| 目标是第三种状态/证据不可读 | 保持 response=null / recovery_required=true，保留证据待修复 |
| outcome 已持久化 | 重放原结果，后续文件变化不改写历史成功 |

旧记录兼容：已完成旧记录继续返回原响应；旧版未完成的非运行写命令没有新发布证据时，保守进入恢复状态，不能仅凭当前文件推断历史执行结果。新记录的证据版本明确区分这一情况。

## 2. C08：操作读取的统一完整性与恢复屏障

实现：[OperationStore](../crates/xray-supervisor/src/operations.rs)、[Supervisor::operation_read](../crates/xray-supervisor/src/lib.rs)。

get/lookup/all 统一检查记录路径与 caller/request ID 的对应关系、事务 ID、请求指纹、响应 request ID 和新版记录的内容哈希。截断 JSON、合法 JSON 中的内容损坏及身份矛盾统一报告 OPERATION_RECOVERY_REQUIRED。重复请求、GetOperation、ListHistory/StageHistory 扫描和启动恢复采用同一阻断规则，保留原证据，并使旧重试失效。

请求自身复用了其他内容的 request ID 时，仍返回 REQUEST_ID_CONFLICT；这不表示已有证据损坏，不会阻断健康 Store。使用新 request ID 的 StopCore/Shutdown 仍可进入安全清理，损坏证据不会被清理动作覆盖。

## 3. C09：Global 的有限就绪等待

实现：[Runtime](../crates/xray-runtime/src/lib.rs)。

HTTP 就绪探测使用 3 s 总期限，对尚未监听、连接重置或提前断开等可恢复连接错误以 50 ms 间隔重试；错误协议响应直接失败。ManagedCore 同时检查对应进程，每 50 ms 检查一次提前退出，不等到总期限耗尽。探测 future 被取消时，其 TCP 连接随之释放。

Global 只有在 HTTP 握手就绪后才继续代理切换与资产提交。真正超时或 Core 提前退出仍交回原事务，恢复旧配置、Core 与代理前态。Direct 尚无全部入站协议就绪验证，继续保留 F03 限制。

IPC 断连不取消已接受写请求；命令仍由 Supervisor 串行完成并保留可查询结果。探测 future 的取消资源管理不改变这一合同。

补充：全量并行回归曾在 Supervisor 关闭后立即重开时遇到目录锁残留。Store 锁现在由专用 guard 在关闭文件之前显式解锁，避免并行 fork/exec 窗口中临时继承的文件描述符延迟释放锁；新增跨平台回归持有复制描述符，验证旧 guard 释放后可重新获取锁，并验证关闭旧描述符不会释放新持有者的锁。

## 4. 正式验证

| 平台 | 结果 |
| --- | --- |
| Linux / Rust 1.94 | 45 项常规回归通过；官方 Core v26.11.9 基础冒烟与受控 A/B 路由 2 项通过 |
| Windows GNU 交叉构建后原生执行 | 24 项常规单元回归通过；真实 Core 候选/历史发布恢复 1 项、IPC 1 项、基础 Core 1 项、A/B 与文件占用 2 项通过 |
| 静态检查 | cargo fmt；Linux / Windows GNU 全目标严格 clippy 通过 |
| Windows release | GUI 收起/独立退出/重开、真实 Core、IPC、Job 强杀清理和无自动重连的历史成功重放通过；IPC 负面验证脚本通过 |

Windows 使用用户提供的 `C:\v2rayN\bin\Xray\xray.exe`（v26.3.27）。事务/路由测试使用模拟系统代理；GUI 冒烟使用 Direct。本轮未更改真实 WinINet 或用户登录任务，未执行远端 CI。

新增/扩展回归：

- [operation_tests.rs](../crates/xray-supervisor/src/operation_tests.rs)：创建/删除发布前后注入、响应丢失后历史结果稳定、外部冲突修复、Accepted/准备阶段区分、三条操作读取路径的截断/有效 JSON 损坏、请求内容冲突不阻断、旧版模糊请求保守恢复，以及 Windows 官方 Core 的候选/历史发布恢复。
- [recovery_tests.rs](../crates/xray-supervisor/src/recovery_tests.rs)：候选校验与历史预览在发布前/后/结算后丢失响应不重复执行，损坏操作记录后仍可停止正在运行的 Core。
- [runtime_flow.rs](../crates/xray-supervisor/tests/runtime_flow.rs)：700 ms 慢启动、5 s 不监听超时与 700 ms 提前退出；失败后联合恢复旧资产、代次、旧 HTTP 端点与模拟代理。
- [Runtime 单元测试](../crates/xray-runtime/src/lib.rs)：延迟监听连接重试与取消后连接释放，均在 Linux/Windows 原生运行。

可重现命令：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
XRAY_TEST_CORE=/path/to/xray cargo test -p xray-supervisor --test real_core --test acceptance_edges --locked -- --ignored
```

```powershell
$env:XRAY_TEST_CORE = 'C:\v2rayN\bin\Xray\xray.exe'
cargo test -p xray-supervisor --lib official_candidate_and_history_publications_recover_on_windows --locked -- --ignored
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\windows-smoke.ps1 -BinaryDir .\target\release -CorePath $env:XRAY_TEST_CORE -Gui
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\windows-ipc-audit.ps1 -BinaryDir .\target\release
```

故障注入在隔离 Store 的明确边界中止发布/结算流程，不能替代所有文件系统/断电/安全软件故障测试。新版 CI 加入 Windows 官方 Core 候选/历史恢复命令，但本机通过不表示远端流水线已执行。

## 5. 交付与剩余边界

更新 `dist/xray-pilot-0.1.0-windows-x64.zip` 与 `dist/SHA256SUMS`，包内包含本记录、设计、实现状态、遗留清单、两份配置示例和 Windows 冒烟/IPC 验证脚本。Core 仍由用户提供。

V01 真实 WinINet、V02 实际登录/权限拒绝、V04–V07 剩余故障/安全/重连矩阵、V08 全交互及 V09 远端 CI 继续待验收。F01 保留/压缩尚未实现，F02/F03 能力受限，F04 诊断与 F05 GUI 队列仍待完善。本轮未扩展至 Phase 3–5。
