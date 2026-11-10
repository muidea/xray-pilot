# C10–C12 代码收口记录

**C10–C12 已修复并通过正式回归，C01–C12 当前全部关闭；Phase 0–2 的其余验收边界仍保留。**

日期：2026-11-10。范围：[当前遗留项复核](phase0-2-current-review.md) 中的 C10/C11/C12，继续保持 Phase 0–2 首版范围。

## C10：独立请求身份与孤立凭据检查

[OperationStore](../crates/xray-supervisor/src/operations.rs) 在新操作主记录之前原子保存 `runtime/request-identities/<caller-request-key>.json`，包含 caller、request ID、请求摘要、transaction ID 和完整性哈希。新操作的 evidence_version 为 2，必须存在相符的独立身份；v0/v1 旧记录继续兼容。

主记录缺失但身份凭据存在时，不将 request ID 当作新请求。启动和普通新写请求登记前检查操作、独立身份、effect/settings/运行事务、归档提交与活动清单的关联；发现孤立证据进入 OPERATION_RECOVERY_REQUIRED，保留证据并阻止普通写入。进程内还记住已见过的旧记录，避免运行期间删除无独立身份的旧记录后误判为未执行。

本轮采用保守阻断：不会凭当前 Profile 状态猜测原操作成功与否，也不会自动重建缺少完整请求/结果的主记录。修复或恢复原始证据后重启，才恢复正常结算；已完成创建请求即使对应 Profile 后来被删除，仍返回原成功结果，不再次创建。使用新 request ID 且携带当前 expected_generation 的 StopCore/Shutdown 继续允许安全清理，不覆盖损坏请求。

独立身份、主记录和所有关联凭据都同时被删除时，无法证明已不存在的历史；本修复不承诺重建全部被移除的数据。旧记录在本轮启动前连同全部关联证据一起丢失时也有这一边界。F01 的未来 GC 必须保留终态身份，不能删除这些新凭据。缺少 installation.json 时也检查独立身份、事务与提交证据，避免仅因主记录目录为空而分配新身份。

## C11：分页摘要、哈希绑定详情与明确超限响应

新增只读合同在 [xray-model](../crates/xray-model/src/lib.rs)，操作视图在 [views.rs](../crates/xray-supervisor/src/views.rs)，响应保护在 [IPC](../crates/xray-supervisor/src/ipc.rs)。协议版本仍为 1，新增命令要求配套本轮 Supervisor。

| 接口 | 行为 |
| --- | --- |
| ListProfileSummaries { after, limit } | 按 Profile ID 排序，每页 1–100 项，仅返回 id/revision 和 next_after；单份正文仍由 GetProfile 获取 |
| GetOperation | 完整序列化记录不超过 1 MiB 时保留原记录格式；超过时返回 Summary，含 request ID、命令名、请求摘要、事务 ID、终态/错误及详情哈希、总字节数、最大块大小 |
| GetOperationDetails { request_id, expected_hash, offset, limit } | 每块至多 1 MiB 的原始 UTF-8 JSON 字节数组；返回 next_offset，完整记录哈希必须与 expected_hash 相同，否则 OPERATION_DETAILS_CHANGED，客户端重新查询摘要 |
| 旧 ListProfiles 与其他超大响应 | 发送帧前检查 8 MiB 上限，返回带原 request ID 的 IPC_MESSAGE_TOO_LARGE；不会只断连。响应超限不撤销已接受的写操作，须查询原 request ID |

持久记录仍包含完整请求与结果；Summary 是传输视图，不是 F01 的终态压缩或详情过期。配置原文仍最多 4 MiB，传输帧仍最多 8 MiB；大量 JSON 转义可能使原文小于 4 MiB 的请求超过帧预算，这时客户端明确拒绝发送。分页按 ID 游标读取当前目录，不是跨页冻结快照；并发增加/删除配置时需刷新列表。

[GUI](../crates/xray-manager-app/src/app.rs) 改用分页摘要，选择配置时读取正文，并保留当前已加载正文作为差异基准。CLI `profiles [AFTER]` 返回一页摘要；`operation ID` 返回操作视图；`operation-details ID` 自动分块获取并校验完整详情哈希。原有 request JSON 入口可直接调用新增只读命令。GUI 可查看成功/失败摘要，完整大记录可通过 CLI 获取。

## C12：统一滑动窗口预算

[RestartBudget](../crates/xray-supervisor/src/restart_budget.rs) 在同一 Supervisor 进程内维护 60 s 滑动窗口，自动恢复尝试开始前统一扣减，最多 3 次。配置/完整性校验失败和实际启动失败都消耗次数；新票据和后续 Core 退出不分配另一份预算。退避依据窗口内已消费次数生成，保持代次、失败实例与 recovery epoch 校验。

预算耗尽后停止当前自动重试并报告 CORE_CRASH_LOOP，日志保留失败提示。窗口到期使额度可再使用，但不会自行复活已取消/耗尽的票据；人工应用/启动仍可用，人工操作不消费或清空自动预算。后续符合条件的新退出事件才生成新票据。停止继续取消旧票据。Supervisor 重启不会延续旧票据或本进程的单调时钟窗口。

## 正式回归与验证

新增/扩展：

- [operation_tests.rs](../crates/xray-supervisor/src/operation_tests.rs)：现代/旧记录、启动/运行期、重复/查询入口的记录缺失阻断，证据修复后历史结果稳定，身份缺失与孤立 Accepted，安全停止/退出，失败恢复的统一预算；Windows 官方 Core 通过端口占用构造“启动失败→成功→再次失败”。
- [recovery_tests.rs](../crates/xray-supervisor/src/recovery_tests.rs)：运行 Core 下记录缺失仍可安全停止；Linux 实际进程在第 2/4 次启动失败，验证混合序列不超过 3 次自动尝试、人工启动不重置预算、窗口到期与停止取消。
- [restart_budget.rs](../crates/xray-supervisor/src/restart_budget.rs)：按每次尝试的实际时间滚动过期，拒绝的尝试不延长窗口。
- [ipc_flow.rs](../crates/xray-supervisor/tests/ipc_flow.rs)：通过独立 Supervisor 创建两份最大 4 MiB 合法配置，摘要分页、完整操作详情逐块还原并核对哈希、过期哈希拒绝、旧接口超限返回明确错误、后续正常通信。

Linux **51 项常规回归**通过，另有官方 Core v26.11.9 的基础/受控 A/B **2 项**验收；格式及 Linux / Windows GNU 全目标严格 clippy 通过。

Windows GNU 构建后原生执行 **28 项常规单元回归**通过，另外通过官方 Core 候选/历史发布恢复及混合失败预算 **2 项**、独立 IPC（含最大配置完整详情）**2 项**、基础 Core **1 项**、A/B 与文件占用 **2 项**。使用用户指定的官方 v26.3.27。大配置 IPC 回归在两端均实际还原全部原请求与原结果，哈希一致。

Windows release 通过真实 Core/IPC、GUI 关闭隐藏/独立退出/重开、Job 强杀清理、无自动连接的历史成功重放及原系统代理不变。release CLI `operation-details` 实际还原两份 4 MiB 字段，记录缺失后的阻断/安全退出/修复重放也通过；IPC 超长/非法/截断/无头、并行客户端和管道占用脚本通过。

复测曾暴露验证脚本的两个判定问题：PowerShell Start-Process 的退出句柄须在等待前获取，才能可靠读取 ExitCode；GUI 启动早期的辅助窗口可能先被识别为主窗口。CLI 经显式句柄确认退出码为 0；[windows-smoke.ps1](../scripts/windows-smoke.ps1) 现等待标题为 Xray Pilot 的可见窗口及稳定句柄，再执行关闭测试，原生复测通过。产品 GUI/CLI 无需为这两项脚本问题改动。

更新 `dist/xray-pilot-0.1.0-windows-x64.zip` 与 `dist/SHA256SUMS`。包内包含本记录、最新设计/实现状态/遗留清单、历史复核与收口记录、配置示例及 Windows 冒烟/IPC 脚本；Core 仍由用户提供。

可重现命令：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
XRAY_TEST_CORE=/path/to/xray cargo test -p xray-supervisor --test real_core --test acceptance_edges --locked -- --ignored
```

```powershell
$env:XRAY_TEST_CORE = 'C:\v2rayN\bin\Xray\xray.exe'
cargo test -p xray-supervisor --lib official_failed_starts_and_successful_restarts_share_budget_on_windows --locked -- --ignored
cargo test -p xray-supervisor --test ipc_flow --locked
```

Windows 使用用户提供的 v26.3.27，不捆绑 Core。预算和事务测试使用模拟代理，release 冒烟使用 Direct；不修改真实 WinINet 或登录任务。新增 Windows 预算专项已接入 CI 配置，远端流水线仍无执行证据。

## 剩余边界

本轮不关闭 V01 真实 WinINet、V02 实际登录/权限拒绝，以及 V04–V09 尚缺的完整故障、安全、GUI 与远端 CI 验收。F01 保留/压缩仍未实现，F02/F03 继续受限，F04 原始诊断及 F05 GUI 队列仍待完善；以 [遗留清单](phase0-2-backlog.md) 为准。
