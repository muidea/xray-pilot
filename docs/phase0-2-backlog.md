# Phase 0–2 遗留项核对

最新核对与 C17 收口：2026-11-10。依据 V1.2 设计、当前工作区源码、已有 Linux/Windows 验证及本轮定向复现。范围保持为用户已选定的 Phase 0–2 首版。

**当前结论：C01–C17 各自确认场景均已关闭并有正式回归，最新为 C17 的 Windows 隐藏窗口托盘命令修复，见 [收口记录](phase0-2-c17-closeout.md)。V03 在受控 A/B 范围保持关闭；其余 8 项 V 验收和 F01–F05 的未完成部分继续保留，Phase 0–2 尚未全部验收。v0.1.1-rc.3 修复托盘退出并将 Windows 默认数据目录调整为程序目录下的 data，见 [版本说明](releases/v0.1.1-rc.3.md)。**

状态含义：

- 已关闭：对应修复或验收条件已满足，并有下文列出的证据。
- 已复现：核对时定向测试观察到与设计不符的结果。
- 静态确认：代码路径存在明确缺口，尚未完成对应故障注入或 Windows 实测。
- 待验收：存在实现或部分模拟测试，缺少目标场景的通过证据。
- 待实现：功能尚未提供；可声明受限交付，但不能勾选对应设计验收项。

P1 表示完成 Phase 0–2 验收前应关闭；P2 表示功能完整性、兼容性或交付完善项。优先级不表示所有待验收项目都已经存在故障。

## 一、确认的代码问题与实现缺口

| ID | 优先级 / 状态 | 触发与影响 | 关闭条件 |
| --- | --- | --- | --- |
| C01 | P1 / 已关闭 | 自动连接关闭时，Supervisor 重启后实际 Core 已停止；随后应用新候选失败，补偿却按照旧清单的 `running=true` 启动旧 Core，未恢复实际前态 | journal 记录事务开始时的实际运行状态；原来停止则回滚后保持停止，原来运行则验证并恢复旧实例对应配置；两类场景均有回归 |
| C02 | P1 / 已关闭 | 未提交事务已经有执行意图，资产恢复遇到外部修改而未完成；`GetStatus` 报恢复未完成，但未完成请求被保存为 `INTERRUPTED_BEFORE_APPLY`，错误地描述为执行前中断 | 操作结果区分未执行、执行后已补偿、恢复未完成；未解决 journal 关联的请求保持 RecoveryRequired；修复后可稳定重建终态，重复请求不再次执行 |
| C03 | P1 / 已关闭 | `SaveSettings` 先修改计划任务，再写 `manager.json`，没有任务前态、执行意图和失败补偿；设置写盘失败或期间崩溃后，实际自启任务可能与设置不一致 | 记录任务所有权与前态，补偿失败时进入恢复状态；覆盖创建/删除成功但设置保存失败、响应丢失与重启恢复 |
| C04 | P2 / 已关闭 | 资源扫描递归检查所有字符串。未知元数据中的 `geosite:just-a-label` 被当成 geosite 文件引用；官方 Core 接受的 JSON 因缺少无关 `geosite.dat` 被管理器拒绝 | 依赖枚举只解释已覆盖原生字段及位置；未知字段保留并降低覆盖能力，不自行赋予文件依赖语义；密码、Tag、备注及未知嵌套字段均有回归 |
| C05 | P2 / 已关闭 | 登录任务名称只绑定 SID；同一用户的不同 `--data-dir` 实例使用同名任务。创建使用 `/F`，一个实例会覆盖另一个；禁用也会删除另一实例的任务 | 任务身份绑定持久安装身份，或明确拒绝第二实例接管；更新/删除前检查任务所有权和目标，不能覆盖外部修改 |
| C06 | P1 / 已关闭 | 应用失败且补偿完成后，写回 `journal.settled` 失败直接返回错误，没有设置写入阻断；旧 journal 仍可能被下一次启动当作未结算事务处理 | 对所有事务结算/提交凭据持久化失败统一阻断后续运行提交；补注入失败用例，证明不会由旧 journal 回退后续已提交资产 |
| C07 | P1 / 已关闭 | 创建/删除 Profile 的文件副作用已完成，但操作响应尚未持久化时重启；恢复只查询运行/设置事务，错误写成 `INTERRUPTED_BEFORE_APPLY`，无法重建已发生的变更结果。ValidateDraft/StageHistory 的非运行副作用也缺少独立结果凭据（静态确认） | 为每种持久写命令保存执行意图和可核验结果证据；区分未执行、已提交、已补偿及恢复中；创建、删除、候选及历史预览均覆盖响应丢失和重启去重 |
| C08 | P1 / 已关闭 | Supervisor 运行期间检测到操作记录 JSON 损坏，lookup/get 的解析错误直接返回，没有设置 blocked；状态仍 `recovery_required=false`，新 request ID 仍能继续写入 | 读取损坏或相互矛盾的操作证据时统一进入 `OPERATION_RECOVERY_REQUIRED`；保留记录并阻断后续普通写入；覆盖重复请求与 GetOperation 两条检测入口，保持安全清理可用 |
| C09 | P2 / 已关闭 | Global 在 Core 启动约 250 ms 后只连接一次 HTTP 端点；探测外层虽有 3 s timeout，但首次 ConnectionRefused 立即结束。700 ms 才监听的有效 Core 在约 300 ms 被拒绝；并行常规测试也出现同类偶发失败 | 在有限就绪期限内重试可恢复的连接失败，核对 Core 仍存活并支持取消；覆盖慢启动成功、真正超时、提前退出及切换失败补偿；Direct 的独立就绪限制仍见 F03 |
| C10 | P1 / 已关闭 | 单条操作文件丢失但 effect 凭据仍在时，启动扫描忽略孤立凭据，旧 request ID 被当作新请求。先创建再删除 Profile，移除原创建记录后重启并重放旧请求，会重新创建已删除文件；Linux/Windows 均复现 | 独立保存并核对请求身份关联；识别孤立事务/发布/提交凭据，重建原结果或保守阻断；覆盖启动及运行期记录丢失，不允许旧请求重新产生副作用 |
| C11 | P2 / 已关闭 | 合法 4 MiB Profile 的 GetOperation 同时包含原请求和完整结果，超过 8 MiB IPC 上限；两份同样配置的 ListProfiles 也超限。服务端丢弃发送错误，客户端只得到响应头 EOF；Linux/Windows 均复现 | 列表分页/摘要与有界操作详情，统一对象和帧预算；超限返回结构化错误，保留完整持久去重证据；补最大配置与聚合回归 |
| C12 | P2 / 已关闭 | crashes 只统计已托管实例退出，启动失败的 Retry.attempt 另行计数。Linux 隔离 fixture 在约 12 s 内发生 4 次自动启动尝试，超过文档声明的每分钟 3 次尝试 | 全部实际自动启动尝试统一计入滑动窗口，含启动失败；覆盖失败/成功/再次退出、预算耗尽及窗口到期，并验证人工启停与票据失效 |
| C13 | P1 / 已关闭 | Global 提交后单独丢失 proxy-lease.json，重启报告 Unmanaged、无恢复错误且继续接受写入，模拟代理仍指向已停止 Core；租约 JSON 损坏则阻止 Supervisor/IPC 启动。Linux/Windows 均复现 | 核对租约与提交/生命周期证据；缺失或损坏时保留可查询恢复状态并阻止普通运行写入；仅凭可信一致证据重建并条件清理，覆盖正常释放、连续切换和外部修改 |
| C14 | P1 / 已关闭 | GUI Reload 读取尚未返回时编辑 B 并校验，旧读取 A 覆盖编辑器，随后 B 候选启用应用；差异预览为空但发出的 ApplyProfile 指向 B。无窗口实际状态处理代码探针复现 | 读取绑定选择/编辑版本，迟到响应不覆盖新草稿；候选及应用绑定当前 Profile、Revision、草稿和验证上下文，覆盖切换、Reload、校验与历史预览 |
| C15 | P2 / 已关闭 | 无请求头 IPC 客户端使服务循环等待 5 s，其间不执行 tick；两平台强杀隔离 Direct Core 超过 3 s 后，后续状态仍返回 Running。Global 退出释放在同一路径的影响为静态确认 | 有界连接收发不阻塞串行协调器的退出检测；补空闲/截断/慢读/不确认客户端与 Core 退出组合，验证状态、条件释放及正常请求可用性 |
| C16 | P2 / 已关闭 | GUI 启动先请求 Profile 列表再读取 Settings；非空列表自动选择首个 Profile，改变 EditorContext，导致 GetSettings 被丢弃。显示空 Core 路径和关闭的自启选项，保存会发送默认值覆盖原设置。Linux / Windows 实际状态处理代码均复现 | 设置读取绑定独立代次与设置编辑版本，不随 Profile/JSON 上下文失效；保留用户已改设置，覆盖已有 Profile 首次加载、空列表、选择变化及迟到读取，并核对保存载荷 |
| C17 | P1 / 已关闭 | Windows 窗口隐藏后 GUI 停止绘制，托盘菜单只在 update 轮询，退出动作和后台关闭响应不能驱动 GUI 退出；重新显示窗口才继续。原生隔离实例已复现 | 托盘回调/IPC 响应主动唤醒隐藏 HWND，显式退出完成关闭确认；在途写操作后只发送一次 Shutdown，失败保留界面及 request ID；原生菜单/后台启动/独立 Core 生命周期验收通过 |

### C17 收口证据（2026-11-10）

Linux 77 项、Windows 原生 GUI 19 项与两平台严格 Clippy 通过。最终 release 的隐藏/可见托盘退出、后台启动完整退出，以及四个入口从其他工作目录共用安装目录下 data 的原生隔离验收通过。此前冒烟的强杀 GUI 不视为托盘退出证据，详见 [C17 收口](phase0-2-c17-closeout.md)。

### C16 收口证据（2026-11-10）

- Settings 读取使用独立请求代次和设置编辑版本，不受 Profile/JSON 变化影响；新读取与已发送保存使旧读取失效，用户修改及改后撤回受保护。
- 正文及历史读取核对选择和草稿，不因 Settings 加载 Core 路径而被误丢弃；候选校验与应用继续绑定完整验证上下文。
- 新增 5 组正式回归，包含非空/空列表启动、两种返回顺序、切换/编辑、迟到读取和保存载荷。Linux/Windows GUI 均通过 10 项状态测试。

详细验证和交付记录见 [C16 收口](phase0-2-c16-closeout.md)。[修复前核对](phase0-2-post-c15-review.md) 保留原复现证据。

### C13–C15 收口证据（2026-11-10）

- C13：独立保存带校验和的租约生命周期凭据；匹配已记录前值时完成中断发布，缺失/损坏/矛盾时提供可查询的 PROXY_RECOVERY_REQUIRED，阻断普通写入并保留安全停止。旧版提交和未结算意图也核对租约，恢复要求不被安全停止抹去；正常释放及补偿回无租约有回归。真实官方 Core 专项使用模拟代理，恢复原始证据后才条件清理。
- C14：读取、校验及历史预览绑定选择代次、Profile/Revision/JSON 和验证上下文；迟到读取保留新草稿，候选及应用再次核对一致性。两平台运行实际 GUI 状态代码的正式回归。
- C15：连接收发和确认移到有界任务，最多 16 个连接、16 项协调队列，运行变更仍串行。空闲、截断、慢读、不确认、满额拒绝及恢复均有回归；Linux fixture 验证 Global 退出后模拟代理释放，Windows release 脚本验证真实 Core 在三种停滞客户端并存时的退出检测。
- F05 队列部分：GUI 最多 32 个待完成请求，状态/日志各只保留一个在途轮询；未发送的写请求不设置 busy。其余编辑器/托盘能力仍待完善。

详细实现、验证记录与限制见 [收口记录](phase0-2-c13-15-closeout.md)。修复前的 [复核报告](phase0-2-post-c12-review.md) 保留历史复现证据，不代表当前仍有相同行为。测试不修改真实 WinINet 或登录任务。

### C10–C12 收口证据（2026-11-10）

- C10：主操作之前原子保存 caller/request ID/请求摘要/事务关联的独立身份，v2 记录强制匹配身份。启动及新普通写请求登记前检查身份、事务、提交与操作记录的一致性；孤立证据保守阻断，修复原记录后才恢复，不猜测缺失历史。覆盖现代/旧版、启动/运行期、查询/重复请求入口，以及运行 Core 的安全停止。全部证据同时被删除的历史仍不可重建，未来 GC 必须保留身份。
- C11：新增每页最多 100 项的 ListProfileSummaries，GUI/CLI 使用摘要读取。超过 1 MiB 的 GetOperation 返回终态摘要及详情哈希；GetOperationDetails 每块至多 1 MiB，按哈希绑定完整 JSON 字节。两份 4 MiB 配置的独立 IPC 回归覆盖完整还原、分页、旧 ListProfiles 超限结构化错误及后续连接；持久记录不压缩、不删除。
- C12：统一 60 s / 3 次自动恢复尝试的滑动窗口，校验/启动失败同样消费；新票据不另配额度。覆盖实际失败/成功/再次失败、人工启动不清空额度、窗口过期及停止取消；Windows 另用官方 Core 与端口占用验证混合失败。

实现和命令见 [收口记录](phase0-2-c10-12-closeout.md)。保守阻断不等于自动补回丢失记录，传输摘要不等于 F01 的终态压缩。

### C10–C12 修复前复核证据（2026-11-10）

详见 [当前复核报告](phase0-2-current-review.md)。45 项常规回归复测通过；C10/C11 在 Linux 当前 debug 和 Windows 最新 release 均复现，C12 在 Linux 确定性进程 fixture 复现。三项场景随后修复并纳入正式回归；本段保留发现问题时的证据。

另外通过真实 Core 的 Direct 异常退出自动恢复：Linux 官方 v26.11.9 与 Windows 用户 Core v26.3.27 均产生新代次和新实例；Windows 新实例的回环 HTTP CONNECT 握手也通过。只强杀隔离测试的 Core，不修改 WinINet 或登录任务。这补充 V07 的正向证据；C12 随后经专项修复关闭，其余重连矩阵保留。

### C07–C09 收口证据（2026-11-10）

- C07：新增 `effect.json`，在 CreateProfile/DeleteProfile/ValidateDraft/StageHistory 的准备工作之前关联请求指纹与事务身份，最终公开文件发布之前记录目标及前态哈希。正式 Profile 的原子写入/删除、候选的 candidate.json 发布分别是单文件提交点；恢复核对前后态并保存可独立重放的结果凭据。准备中断与发布前中断返回已补偿；目标已发布重建成功；第三种状态保留待恢复。结算/结果无法持久化阻断新写入。候选私有准备副本不自动回收，仍按 F01 保留。
- C08：OperationStore 在 get/lookup/all 统一校验记录路径、调用者、请求/响应身份、请求摘要和新版记录哈希；Supervisor 的重复请求、GetOperation、历史读取与启动扫描统一阻断损坏证据。请求内容与已登记内容不同仍返回 REQUEST_ID_CONFLICT，不阻断健康 Store；安全停止使用新 request ID 继续可用。
- C09：Global HTTP 探测在 3 s 总期限内每 50 ms 重试可恢复的连接错误，同时监测对应 Core 是否提前退出；中止探测 future 会关闭连接。回归覆盖 700 ms 慢启动、5 s 不监听超时、700 ms 时提前退出、取消连接、旧 Core/资产/模拟代理联合恢复，以及 Windows 原生的延迟监听重试。
- 兼容性：已完成旧记录可继续重放；旧版未完成的非运行写请求若没有发布证据，保守保留 RECOVERY_REQUIRED 等待核对，不推断执行前中断。新版请求使用独立发布凭据恢复，不重做写操作。

实现：[effects.rs](../crates/xray-supervisor/src/effects.rs)、[operations.rs](../crates/xray-supervisor/src/operations.rs)、[Supervisor](../crates/xray-supervisor/src/lib.rs)、[Runtime](../crates/xray-runtime/src/lib.rs)。回归：[operation_tests.rs](../crates/xray-supervisor/src/operation_tests.rs)、[恢复测试](../crates/xray-supervisor/src/recovery_tests.rs)、[runtime_flow.rs](../crates/xray-supervisor/tests/runtime_flow.rs)。

### C01–C06 修复与回归证据

- C01：journal 新增实际 `previous_instance`，记录前先检查持有进程是否已退出。补偿只在前态确有活实例时重启旧配置；旧版 journal 缺字段时保守地不重启。正式回归同时覆盖原来 Running 与重开后 Stopped。
- C02（运行事务恢复范围）：操作记录新增可恢复的 `recovery_required` 标记，未解决的执行意图保持 `response=null`。重复请求返回 `RECOVERY_REQUIRED`，不会再次执行；修复冲突并重启后重建 `INTERRUPTED_COMPENSATED`。只有无意图、无提交证据才归类 `INTERRUPTED_BEFORE_APPLY`；已有提交仍返回原成功结果。
- C03：`StartupController` 在副作用前写 `settings.json` 事务，保存设置与任务前态/目标，读回检查并条件补偿。`manager.json` 是设置发布点，结算结果持久保留以重建丢失响应；结算/补偿未完成时 Supervisor 阻止后续修改。回归覆盖创建/删除成功但设置发布失败、补偿失败后重启、执行意图中断、成功响应丢失及后续设置覆盖。
- C04：仅在原生 routing rule 的 domain/ip/source 数组，以及入站/出站 TLS certificates 的 certificateFile/keyFile 枚举依赖。密码、Tag、备注、未知嵌套文件字段不触发复制；未知字段仍由覆盖白名单降低自动恢复能力。真实 geo/ext/证书冻结与未知字段均有正式回归。
- C05：任务名称绑定 SID、持久 installation ID 和规范化数据目录。保存本实例预期任务定义，写入/删除前重新查询并比对完整定义；不使用无条件覆盖。Task Scheduler 的默认值、用户身份表示及生成 URI 在比较前统一处理。不同安装/目录的名称隔离、外部修改保留、连续禁用均有回归。不会自动接管此前缺少所有权记录的同名任务。
- C06：统一 journal 结算错误的写入阻断；登记、初始意图、操作响应及提交记账失败也阻止继续写入。注入补偿后的结算失败，验证新应用被拒绝，重启结算后才允许新提交。恢复也检查归档提交凭据，安全停止不会使旧成功事务被误当成未提交事务。

主要实现：[Supervisor](../crates/xray-supervisor/src/lib.rs)、[OperationStore](../crates/xray-supervisor/src/operations.rs)、[StartupController](../crates/windows-integration/src/startup.rs)、[Task Scheduler 后端](../crates/windows-integration/src/startup.ps1)、[资源扫描](../crates/xray-config/src/resources.rs)。

正式回归：[Supervisor 恢复](../crates/xray-supervisor/src/recovery_tests.rs)、[自启事务与 Windows 原生任务](../crates/windows-integration/src/startup/tests.rs)、[真实 Core](../crates/xray-supervisor/tests/real_core.rs)。

### 上轮修复前的复现证据

C01：先成功应用配置，退出并重新打开 Supervisor（`connect_on_start=false`），确认实际 Stopped；再应用通过 `-test` 但启动失败的候选。期望 Stopped，实际得到 **Running**。

C02：模拟已登记请求和资产提升意图，正式资产出现第三种哈希；重启后确认 `recovery_required=true`，但关联请求的错误码实际为 **INTERRUPTED_BEFORE_APPLY**，没有保持恢复未完成语义。

C04：以下配置通过真实官方 Xray v26.11.9 的 `run -test`，但在空资源来源目录中执行 `resources::freeze` 返回 **IO_ERROR / No such file or directory**：

```json
{
  "outbounds": [{ "tag": "direct", "protocol": "freedom" }],
  "metadata": { "note": "geosite:just-a-label" }
}
```

以上为修复前的核对证据：3 个定向断言当时均未通过，揭示原有 19 项常规回归未覆盖的场景。对应场景现已转为正式回归并通过。核对探针使用隔离数据与模拟代理；收口时另以唯一任务名验证 Windows Task Scheduler，测试结束已删除该测试任务。

## 二、验收证据与仍缺内容

| ID | 优先级 / 状态 | 项目 | 当前证据与仍缺内容 |
| --- | --- | --- | --- |
| V01 | P1 / 待验收 | 真实 WinINet 与原代理恢复 | 模拟后端覆盖根快照和冲突；尚未实测真实四项 WinINet 设置往返、企业 PAC、连续 Profile/端口切换、外部修改和异常后条件释放 |
| V02 | P1 / 部分通过 | Task Scheduler | 已实测当前用户创建/查询/禁用、含空格/中文的数据路径、外部修改保护与幂等禁用；仍缺权限拒绝、GUI/独立 Supervisor 在实际登录时的入口行为验证 |
| V03 | P1 / 已关闭（受控上游） | A/B 真实流量与禁止回退 | Linux 官方 Core v26.11.9 和 Windows 用户 Core v26.3.27 均通过：真实 HTTP 隧道请求中 example.com/其子域收到 B 响应，其余域名收到 A 响应；B 返回 SOCKS 拒绝或停止监听时 A 的连接记录均未增加。使用真实 Core 与两个本地受控 SOCKS 上游，不代表已验收公网账号/TLS/远端出口 |
| V04 | P1 / 部分通过 | 持久化边界故障注入 | C01–C09 均有恢复/结算/损坏记录回归，非运行发布增加提交点前后注入。C10 的记录缺失场景已修复并覆盖；C13 的租约缺失/损坏已修复并覆盖。仍缺每个副作用/发布/记账边界的系统化退出矩阵，未完成真实断电测试 |
| V05 | P1 / 部分通过 | Windows 文件系统原子发布 | 已用真实 Windows 文件句柄禁止替换正式 Profile，验证应用失败后旧资产、代次、真实 Core HTTP 端点和模拟代理联合恢复；仍缺磁盘满、安全软件干扰及断电后的恢复判定 |
| V06 | P1 / 部分通过 | ACL / Named Pipe 负面访问 | Windows 已通过超长帧、非法 JSON、截断帧、无请求头超时后的后续正常连接，两个同时连接客户端的响应身份，以及已占用管道端点拒绝接管。合法大响应的 C11 已通过摘要/分块及明确超限错误修复；C15 补齐停滞客户端与退出检测、并发上限回归；仍缺不同 SID/远端拒绝、恶意服务端身份及更高压力验证；同 SID 正向测试不能替代跨用户测试 |
| V07 | P1 / 部分通过 | 自动重连与历史回滚 | 常规回归覆盖 connect_on_start=true、新代次、历史预览/重新应用、冻结 geo 缺失时停止连接。Linux/Windows 真实 Core 的 Direct 异常退出后新代次/实例恢复已通过，Windows 新 HTTP 端点握手通过；文件占用覆盖 Global 失败后的旧端点/模拟代理恢复。混合启动失败预算 C12 已修复，Linux fixture 与 Windows 官方 Core 专项通过；仍缺更多失败/完整性/停止取消组合与真实 Global 联测；C09 的慢启动/超时/提前退出回归保留 |
| V08 | P2 / 部分通过 | GUI 与托盘全交互 | 窗口启动、关闭隐藏、只退出 GUI 保持 Core、重开已有证据；C14 的迟到读取及预览/候选一致性已修复，GUI 状态回归已通过。C17 已补原生托盘 Open、隐藏/可见 Exit GUI 与后台启动 Quit；其他托盘逐项、导入/导出、校验/差异/应用、快照回滚、重连和未知结果查询仍需完整检查，尤其耗时/断连期间的交互；设置加载问题 C16 已修复并补正式回归；F05 的其余编辑器/托盘能力保留 |
| V09 | P2 / 部分通过 | 远端 CI 与交付复测 | v0.1.1-rc.1 远端 Ubuntu/Windows MSVC 流水线已通过。v0.1.1-rc.2 本机 Linux 74 项、Windows 原生 52 项回归和两平台严格 Clippy 通过；官方依赖实际下载/校验/复用/修复与离线失败保护通过；最终 release 使用受管官方 Core 的 GUI/Core/IPC/Job 和停滞客户端监督冒烟通过。本轮首次大配置 IPC 曾失败、单独与完整复测通过，原因尚未确定；rc.2 发布标签远端 Ubuntu/Windows MSVC 已通过；rc.3 已补本机 Linux 77 项、Windows GUI 19 项、最终 release 托盘/目录验收，rc.3 远端与更广压力仍待确认 |

## 三、尚未完成的能力及已声明限制

| ID | 状态 | 事项与处理原则 |
| --- | --- | --- |
| F01 | 待实现 | 资源/Core/候选的引用保留与垃圾回收，以及 OperationRecord 终态压缩/详情过期。当前全部保留，去重证据不会被主动删掉，但不满足保留策略/压缩验收；实现时先证明不会清理未完成恢复或复用旧请求身份 |
| F02 | 部分实现 | 依赖覆盖白名单不包含 DNS、复杂传输和全部原生字段；绝对文件路径要求显式修改。可继续以受限能力交付，必须清楚展示限制；C04 误判不能被当作正常限制 |
| F03 | 部分实现 | Direct 仅检查通过 Core 校验且未立即退出，没有所有入站的协议就绪检测；Global 仅证明本地 HTTP 握手，不证明远端出口 |
| F04 | 待完善 | Core `-test` 失败只返回退出码和通用错误，原始诊断没有传给内存日志；需提供可用且不会把秘密扩散到持久错误记录的诊断路径 |
| F05 | 部分完善 | GUI 已限制最多 32 个待完成请求，并合并状态/日志在途轮询；读取及候选一致性 C14 已关闭。出站独立显示名称、结构化字段增删、数值编辑、托盘 Profile 列表等仍未完善，全部 GUI 交互验收继续归 V08 |

PAC 生成与服务属于 Phase 3；应用安装器/升级/自动更新和长期运维产品化属于 Phase 4；TUN/Helper/路由/DNS 特权恢复属于 Phase 5。这些仍未实现，但不计入本轮 Phase 0–2 的缺陷关闭清单。

## 四、后续顺序

1. 完成 V01 真实 WinINet 与 V02 实际登录/权限拒绝验证，补齐 V04–V07 的系统故障、安全与重连矩阵。
2. 明确 F01 的保留/压缩交付范围，完善 F04/F05，完成 V08/V09；F02/F03 的能力边界继续明确展示。

C07–C12 的收口记录保留为既有修复证据；当前遗留以此清单和 [C17 收口记录](phase0-2-c17-closeout.md) 为准；已关闭项的证据见各轮收口记录。
