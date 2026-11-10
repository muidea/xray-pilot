# Phase 0–2 当前遗留项复核

日期：2026-11-10。基线：C07–C09 收口后的工作区源码、最新 Windows release 包与既有验证记录。范围保持 Phase 0–2。

**修复前复核：本轮新增 C10 / P1、C11 / P2、C12 / P2，均有定向复现；核对当时尚未修改产品代码。C01–C09 的既有修复保留，常规 45 项回归复测通过，但不能覆盖本报告的新边界。当前状态以 [遗留清单](phase0-2-backlog.md) 为准。**

**后续收口：C10–C12 已修复，复现场景已纳入正式回归；见 [收口记录](phase0-2-c10-12-closeout.md)。以下保留修复前步骤与观察，不表示当前代码仍有相同缺陷。**

## 1. C10 / P1：操作记录文件丢失后，残留事务凭据没有阻止旧请求重新执行

位置：[OperationStore::get / register / all](../crates/xray-supervisor/src/operations.rs)、[Supervisor::recover](../crates/xray-supervisor/src/lib.rs)、[EffectJournal](../crates/xray-supervisor/src/effects.rs)。

get 将不存在的记录作为 None；all 只枚举现存操作文件。恢复非运行发布凭据的入口依赖这些操作记录，未检查是否存在没有对应操作记录的 effect/settings/运行事务。新 effect 凭据保存 transaction ID 和 request digest，缺少独立的 caller/request ID 关联，无法直接重建丢失的请求身份。

隔离复现步骤（Linux 与 Windows 原生均通过复现）：

1. 请求 A 创建 Profile `p`，随后请求 B 删除 `p`；两个请求均成功，正式文件已不存在。
2. 关闭 Supervisor，仅移除 A 的 `runtime/operations/<key>.json`，保留 installation、B 的记录以及 A 的完整 `effect.json`，模拟单条操作证据丢失。
3. 重启并提交原始 A，保持 request ID 与全部请求内容不变。

两端观察一致：

```text
startup recovery_required: false
profile count before replay: 0
replay original A: success
profile count after replay: 1
```

旧请求被登记为新事务，再次创建已经被后续请求删除的文件。此问题要求操作记录文件丢失；当前尚未实现 GC，正常操作不会主动删除这些记录。本次是存储缺失的构造故障，不是已完成真实断电验证，也不是允许用户删除证据来绕过恢复。

这与 C07 的“记录还在、结果未保存”及 C08 的“记录存在但读取损坏”不同。关闭要求：持久化可独立核对的请求身份关联；恢复时识别孤立事务/提交/发布凭据，能够重建则返回原结果，不能判定则进入 OPERATION_RECOVERY_REQUIRED。补齐启动与运行期记录丢失场景，并证明旧请求不会重新产生副作用；安全清理仍可用。

## 2. C11 / P2：合法配置和聚合读取超过 IPC 响应上限时直接断连

位置：[parse_config](../crates/xray-config/src/lib.rs)、[IPC::send / serve](../crates/xray-supervisor/src/ipc.rs)、[GetOperation / ListProfiles](../crates/xray-supervisor/src/lib.rs)。

单份原生 JSON 允许至多 4 MiB；IPC 单帧上限为 8 MiB。创建操作的结果包含完整 Profile JSON，GetOperation 又同时返回完整原请求及结果，两份 JSON 加上记录元数据即可超过帧限制。ListProfiles 返回全部 Profile 正文，同样没有聚合容量限制或分页。

Linux 与 Windows 原生均复现：

1. 创建内容为 `{}` 加空白、总长 **4,194,304 字节**的合法配置，成功收到创建结果。
2. GetOperation 查询该请求：服务端 send 返回 IPC_MESSAGE_TOO_LARGE，serve 丢弃发送错误并关闭连接；客户端在读取响应长度时得到 EOF，没有收到结构化错误。
3. 再创建一份同样大小的合法 Profile，ListProfiles 也在读取响应头时 EOF。
4. 后续 GetStatus 正常，recovery_required=false；Supervisor 没有崩溃。

此问题不要求磁盘损坏。配置数量增长后，即使每份远小于 4 MiB，也可能触发列表总大小限制；GUI 初始化依赖 ListProfiles，可能因此无法正常列出配置。操作结果查询失败也妨碍写请求结果不明后的核对。

关闭要求：为列表提供分页/摘要，为操作查询提供有界结果或详情读取，并统一明确单对象与帧预算；任何超限应返回可识别错误，不能只断连。保留完整持久去重证据，不能通过删掉请求正文或原结果来规避帧限制。补单份最大配置、多个配置聚合及超限后继续正常通信的回归。

## 3. C12 / P2：启动失败的重试没有计入每分钟统一预算

位置：[Supervisor::tick](../crates/xray-supervisor/src/lib.rs)，特别是 crashes 队列与 Retry.attempt 两个计数；既有 [重启预算测试](../crates/xray-supervisor/src/recovery_tests.rs)。

crashes 只在 tick 观察到已托管实例退出时增加；一次自动重启中的启动失败由 Retry.attempt 单独限制为三次。两个计数没有统一，因此实现无法保证 README、实现状态及 CORE_CRASH_LOOP 错误中声明的每分钟三次自动重启尝试。既有测试覆盖每次重启成功后再退出，没有覆盖重试期间启动失败。

Linux 确定性进程 fixture 复现：版本/配置校验正常，首次实际启动成功；实际启动序号 2、4 提前退出，其余保持运行。对本测试 Supervisor 的运行子进程注入两次退出，等待调度器自动恢复：

```text
initial starts: 1
automatic restart launches: 4
elapsed: 12.19 seconds
final core_state: Running
recovery_required: false
```

四次尝试中两次启动失败、两次成功。测试只作用于隔离子进程，未使用真实代理或登录任务；此预算复现尚未在 Windows fixture 上运行。共享调度代码静态路径相同。

关闭要求：明确并按统一滑动窗口记录每次实际自动启动尝试，启动失败也消耗预算；耗尽后停止自动尝试并报告原因，人工启停与旧票据失效规则保持明确。补“失败、成功、再次退出”的混合序列及窗口到期回归，不能只统计已经成功运行过的进程退出。

## 4. 验证证据与剩余边界

| 范围 | 本轮证据 | 仍缺内容 |
| --- | --- | --- |
| 常规回归 | cargo test --workspace --locked：45 项通过 | C10–C12 当前只有定向核对探针，尚未转成修复后的正式回归 |
| C10 / C11 | Linux 当前 debug Supervisor 与 Windows 最新 release 包均复现 | 产品修复及正式跨平台回归 |
| C12 | Linux 确定性 fixture 复现超预算 | Windows fixture 与修复后的统一预算验证 |
| V07 真实 Core | Linux 官方 v26.11.9 与 Windows 用户 Core v26.3.27：强杀隔离的 Direct Core 后，自动产生新代次/实例并恢复 Running，分别约 5.14 s / 2.91 s；Windows 新实例的回环 HTTP CONNECT 握手通过 | 全部异常退出、失败重试、停止取消与完整性组合及真实 Global 联测；C12 也须先关闭 |
| V01 / V02 | 沿用前次证据，本轮未修改 WinINet 或注册登录任务 | 真实代理往返/外部竞争、实际登录与权限拒绝 |
| V04 / V05 / V06 | 新增记录缺失与合法大帧边界发现；前次提交点注入、Windows 文件占用和管道负面测试证据保留 | 完整退出矩阵、磁盘满/断电、安全软件、跨 SID/远端及压力 |
| V08 / V09 | 静态核对 GUI 与 CI 配置；前次 GUI/Core/IPC/Job 包验证证据保留 | GUI 全交互、F05 队列和远端 Ubuntu/Windows MSVC CI |

F01 的保留/回收/终态压缩仍未实现；F02 的覆盖白名单与 F03 的 Direct 就绪范围仍受限；F04 的 `run -test` 原始诊断仍被丢弃；F05 的无界 mpsc 队列仍会在读 IPC 等待时积压轮询。没有据既有通过测试关闭这些能力项。

本轮探针及日志位于本机 `/tmp/xray-pilot-current-audit.py`、`/tmp/xray-pilot-retry-audit.py`、`/tmp/xray-pilot-current-audit*.log`、`/tmp/xray-pilot-retry-audit.log`、`/tmp/xray-pilot-official-retry-audit-windows.log`。Windows 构造数据在 `C:\Users\muidea\AppData\Local\Temp\xray-current-audit-ec0b4be402f04367ae8eb96f255c4e34`，官方 Core 恢复数据在同级 `xray-current-audit-4691be1df9bd4795bda8f20d4852dbd2`；探针位于前次验证目录的 `audit-current.ps1` 和 `audit-official-retry.ps1`。步骤与关键观测已完整记录在本文，不依赖临时文件长期存在。

核对当时未修改产品行为，未重新打包；现有 zip 仍是 C07–C09 收口版本，不包含本轮新增核对文档。该次核对建议先关闭 C10–C12，再补 V01/V02 与其余系统验收，并按 F01/F04/F05 的交付范围继续收口。
