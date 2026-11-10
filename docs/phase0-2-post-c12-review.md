# Phase 0–2：C10–C12 收口后遗留项复核

核对日期：2026-11-10。范围为用户已选定的 Phase 0–2 首版，基线为当前工作区及 C10–C12 收口后的 Windows release。本文记录修复前核对结果；核对时尚未修复下列产品行为，后续收口状态见下方说明。

**新增 C13 / P1（租约证据缺失或损坏）、C14 / P1（GUI 预览与候选不一致）、C15 / P2（IPC 客户端等待暂停退出检测）。C01–C12 的原关闭范围保留；Phase 0–2 仍未全部验收。**

**后续收口：C13–C15 已修复并纳入正式回归，见 [收口记录](phase0-2-c13-15-closeout.md)。下文保留修复前的观察与探针，不表示当前产品仍有相同行为。**

## 1. C13 / P1：租约文件丢失后静默放弃恢复，损坏时无法启动诊断入口

代码位置：[ProxyController::open](../crates/windows-integration/src/lib.rs:39)、[Supervisor 初始化](../crates/xray-supervisor/src/lib.rs:87)、[启动恢复](../crates/xray-supervisor/src/lib.rs:932)。

`ProxyController::open` 将缺失文件解释为无租约；`release` 遇到无租约直接成功。已结算运行 journal 在恢复时被跳过，已有 Global 提交事实没有触发租约缺失检查。租约 JSON 无法解析则在 Supervisor 构造之前通过 `?` 返回，未能进入可查询的 RecoveryRequired 状态。

隔离复现步骤：

1. 使用模拟代理后端，初始快照包含企业 PAC；通过官方 Core 校验并应用 Global，确认模拟代理已指向该 Core 的 HTTP 入站。
2. 丢弃 Supervisor，让托管 Core 停止；仅删除 `runtime/proxy-lease.json`，保留清单、journal、操作记录、独立身份及提交证据。
3. 重新打开 Supervisor 并读取状态，然后用新 request ID 创建 Profile。
4. 再次关闭，将租约文件改为截断 JSON `{`，重新打开。

Linux 官方 Core v26.11.9 与 Windows 用户 Core v26.3.27 得到相同结果：

```text
core_state=Stopped
proxy_state=Unmanaged
recovery_required=false
last_error=null
dead_proxy_retained=true
new_write_accepted=true

corrupt lease: Supervisor construction -> CONFIG_PARSE_FAILED
dead_proxy_retained=true
```

影响：租约文件单独丢失时，失效代理没有恢复且没有告警；后续接管还可能把失效代理当成新的根快照。损坏文件则阻断 IPC 服务启动，GUI 无法通过 GetStatus 获取恢复状态。此故障注入证明恢复证据缺口，不表示观察到了真实断电导致文件丢失，也不代表已经验证真实 WinINet 行为。

关闭条件：

- 结合运行提交、事务及租约生命周期证据，区分正常无租约与应有证据缺失，不能仅凭历史上用过 Global 就判定所有后续无租约状态异常。
- 证据不足时保守进入可查询的恢复状态，阻止普通运行写入，保留原始证据与可用的安全停止；不能猜测根快照或覆盖外部设置。
- 只有足够可信且一致的独立证据才允许重建租约并条件恢复；补充文件丢失、截断、正常释放、连续切换与外部修改回归。

## 2. C14 / P1：迟到的 Profile 读取使预览内容与应用候选不一致

代码位置：[读请求不设置 busy](../crates/xray-manager-app/src/app.rs:158)、[GetProfile 无条件 load](../crates/xray-manager-app/src/app.rs:217)、[校验结果直接启用候选](../crates/xray-manager-app/src/app.rs:244)、[预览与应用](../crates/xray-manager-app/src/app.rs:449)。

读请求发出后编辑器仍可修改；响应没有绑定选择代次或编辑版本。串行 IPC worker 保证响应顺序，却没有防止旧读结果覆盖新草稿。验证响应也没有重新核对当前草稿是否仍与候选一致。

无窗口探针直接运行当前 `App::send`、`receive`、`load`、`diff_lines`，按实际 worker 的串行顺序注入合法响应：

1. 当前正式配置为 A；点击 Reload，GetProfile 尚未返回。
2. 将编辑器修改为 B，发送 ValidateDraft(B)。此前读取是只读请求，未阻止这两步。
3. GetProfile(A) 先返回，`load(A)` 覆盖编辑器及 saved_json。
4. ValidateDraft(B) 返回，候选变成 B，应用按钮被启用。
5. 当前差异预览比较 A 与 A，得到空差异；应用请求却携带 B 的 candidate ID/digest。

探针输出：

```text
editor_is_old_a=true
candidate_is_new_b=true
preview_diff_lines=0
apply_enabled=true
apply_queues_candidate_b=true
```

影响：迟到读取会丢失未保存修改；上述组合还使用户确认的预览与即将应用的配置不同。此项是实际 GUI 状态处理代码的确定性复现，使用模拟 IPC 响应；尚未用 Windows 窗口自动化重现完整点击过程，也没有实际提交 B 到 Core。

关闭条件：为 Profile 读取绑定选择/草稿版本，阻止迟到响应覆盖新编辑；候选结果和应用按钮须绑定当前 Profile、Revision、草稿及验证上下文。覆盖读取期间编辑、切换 Profile、Reload、校验及历史预览的交互组合。仅限制队列长度不能关闭此项。

## 3. C15 / P2：等待 IPC 请求头会暂停 Core 退出检测

代码位置：[Unix 服务循环](../crates/xray-supervisor/src/ipc.rs:143)、[Windows 服务循环](../crates/xray-supervisor/src/ipc.rs:194)、[Core 退出检测与释放](../crates/xray-supervisor/src/lib.rs:1070)。

服务循环在 `select!` 的连接分支内等待完整请求、处理响应及 Windows 接收确认。这个分支返回之前不会再次执行 `interval.tick()`；异步 I/O 本身没有让同一循环内的 Supervisor::tick 并行运行。单连接的 5 s 超时只能限制一次等待，不能保证这期间的监督继续工作。

Linux 当前 debug 与 Windows C10–C12 release 均使用隔离 Direct Core 复现：

1. 应用真实官方 Core，确认 Running。
2. 同一用户连接 IPC，不发送任何请求头，等待 200 ms 使服务端进入读取。
3. 强制结束该隔离 Core；3.2 s 后用另一客户端查询状态。

| 观察 | Linux v26.11.9 | Windows 用户 Core v26.3.27 |
| --- | --- | --- |
| 第二客户端额外等待 | 1.601 s | 1.575 s |
| 首次状态 | Running，实例 ID 仍存在 | Running，实例 ID 仍存在 |
| 最终新代次恢复，距强杀 | 10.483 s | 7.596 s |

Linux 在强杀 3.2 s 后还观察到原子进程为 zombie，说明尚未经过退出收取。最终均恢复了新代次，因此本次证明的是监督暂停及陈旧状态，不是永久无法重连。两平台耗时含各自 Core 校验/启动开销，不作为性能基准。

Global 的退出后代理释放也位于同一个 tick 路径，静态确认会受此等待影响；本轮未通过真实 WinINet 复现该影响。Windows 响应确认等待与慢响应发送存在同类串行等待结构，本轮只定向验证无请求头分支。同 SID 的该结果不等于跨用户访问绕过。

关闭条件：将有界连接接收/发送与串行状态变更协调分开，使空闲/截断/慢读/不确认客户端不暂停 Core 退出检测及条件释放；保留写事务串行化，并补充退出时的正常客户端可用性与排队上限验证。

## 4. 核对证据及现有遗留

本轮只重建 Linux Supervisor 并运行定向探针，没有重跑整套 51 项常规回归；既有通过结果见 [C10–C12 收口记录](phase0-2-c10-12-closeout.md)。核对探针的断言用于确认观察到的问题，不计为新场景验收通过。

原始日志位于本机：

- `/tmp/xray-pilot-latest-lease-review.log`、`/tmp/xray-pilot-latest-lease-windows-review.log`：模拟代理、两平台真实 Core 的缺失/损坏租约。
- `/tmp/xray-pilot-latest-gui-review.log`：当前 GUI 状态处理代码的无窗口探针。
- `/tmp/xray-pilot-latest-ipc-review.log`、`/tmp/xray-pilot-latest-ipc-windows-review.log`：两平台真实 Core、空闲客户端及最终恢复。
- 探针源文件保存在 `/tmp/xray-pilot-latest-review-probes/`，不加入默认测试发现；临时测试文件已从 crate 移除。

全部测试使用隔离数据。未改动真实 WinINet 或登录任务；本轮启动的 Supervisor/Core 已清理。产品源码、正式回归及 release 程序包未修改，本次只刷新核对文档；现有程序包未包含本报告。

仍待完成的 8 项验收为 V01、V02、V04–V09；V03 仅在受控 A/B 范围保持关闭。F01–F05 继续保留。C13 对应 V04/V05 的已确认缺口，C14 对应 V08/F05，C15 对应 V06/V07；不能以既有部分通过证据关闭本轮新增场景。

建议先关闭 C13/C14，再处理 C15 与 F05 的队列边界；随后继续真实 WinINet、实际登录/权限拒绝、故障矩阵、GUI 全交互和远端 CI 验收。F01/F04 仍需明确实现或受限交付范围，Phase 3–5 不纳入本轮关闭范围。
