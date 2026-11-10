# Phase 0–2：C13–C15 收口记录

日期：2026-11-10。对应 [修复前复核](phase0-2-post-c12-review.md) 与 [遗留清单](phase0-2-backlog.md)，范围保持为 Phase 0–2 首版。

## C13：代理租约证据与可查询恢复状态

[ProxyController](../crates/windows-integration/src/lib.rs) 新增独立的 `runtime/proxy-lease-evidence.json`，保存版本、完整租约意图、前值摘要、恢复要求及校验和。新租约带 `evidence_version=1`；旧租约缺此字段时仍可读取。

每次租约持久化按以下顺序进行，调用者只有在全部成功后才能继续 Windows 副作用：

1. 原子保存独立意图及主文件前值摘要。
2. 原子发布主租约。
3. 将前值摘要收紧为刚发布的值，关闭中断发布的兼容窗口。

启动时，主文件须匹配独立意图或其明确记录的前值。匹配前值时可完成被中断的发布；正常完成后主文件再丢失，不能借旧前值误判为首次接管尚未发生。文件缺失、截断、空值替换、内容修改、独立凭据缺失或损坏都会保留 `PROXY_RECOVERY_REQUIRED`。Supervisor 仍可打开 IPC、读取状态、停止 Core 和退出，普通写入被阻断；损坏原文件不被新租约覆盖，也不猜测原代理。

旧版已提交 Global 也要求租约。未结算旧事务若有前次持有证据，或 Global 意图下实际代理已偏离事务前态，同样进入恢复状态；尚未写代理且实际状态仍等于前态的旧事务可正常结算。独立恢复标记使安全停止、提交 Stopped 和再次重启都不能抹去“缺少租约”的事实。正常 Released、补偿回无租约及从未接管的数据目录不会因此被误判。

此版本保守处理丢失/损坏证据，不自动从其他历史记录猜测根快照。修复时先停止 Supervisor、保留损坏文件，再恢复可信且互相一致的原主租约与独立凭据，重启后核对状态。旧版原本没有独立凭据时，须在人工核对原租约后处理本次生成的 `required=true` 标记，不能仅删除标记或新建空租约来解除阻断。全部证据同时消失的历史不承诺重建。

Windows 实测曾在原子替换时返回 OS error 5。[atomic_write](../crates/xray-config/src/lib.rs) 对替换阶段的 5/32/33 错误增加最多 500 ms、每次 10 ms 的有限重试，保持原子替换，不放宽权限或退回覆盖写。持续占用仍返回失败并进入既有补偿；这不代表已完成 V05 的磁盘满、安全软件和断电矩阵。

## C14：GUI 草稿、读取及候选绑定

[App](../crates/xray-manager-app/src/app.rs) 为请求保存选择代次、选中/已加载 Profile、Revision、JSON 摘要、Core 路径、资源来源、代理模式及 HTTP Tag。读取、校验和历史预览的响应只有在上下文仍一致时才能修改当前草稿。新的选择淘汰旧响应；加载期间发生编辑则保留编辑，并将可见选择保持在该草稿所属 Profile。

候选必须匹配当前 Profile、基础 Revision 及 JSON 哈希；应用入口还检查候选 ID/digest 和验证上下文。历史预览先装载历史 JSON，再绑定对应候选。任何相关内容改变后都需重新校验，避免“预览 A、应用 B”。

同时收口 F05 的队列部分：GUI 最多保留 32 个待完成请求，状态/日志轮询各只保留一个在途请求；发送使用有界非阻塞队列。队列满或断连时明确报告未发送，写请求不会错误设置 busy。F05 中的结构化数值/字段编辑、显示名称和托盘 Profile 列表继续保留。

## C15：IPC 传输与运行协调分离

[IPC](../crates/xray-supervisor/src/ipc.rs) 用独立连接任务处理帧读取、响应发送及 Windows 接收确认。Supervisor 的唯一协调循环接收完整请求并串行执行运行变更，同时处理退出检测与重试；空闲或慢客户端不再占用这个循环。

- 最多 16 个活动连接任务；协调请求队列上限为 16。
- 连接读取、响应发送、Windows 接收确认各限 5 s。
- 满额连接直接拒绝；完整请求无法入队时返回 `IPC_BUSY`，该请求未被协调器接受。
- 退出检测优先处理到期 tick；连接任务完成后及时回收，退出时有界等待 shutdown 响应再清理连接。
- Windows 仍使用新管道实例、连续保持管道名所有权，并保留 ACL、拒绝远端及双端身份核查。

长事务本身仍在协调器内串行执行，本轮没有实现业务操作的抢占/取消。该边界与连接 I/O 等待分别记录，不能据此关闭所有 V06/V07 压力和故障组合。

## 正式回归与验证

- [lease_recovery_tests.rs](../crates/xray-supervisor/src/lease_recovery_tests.rs)：缺失/截断/null/内容修改/独立凭据异常，查询与安全停止，修复后条件清理；旧版提交及执行中意图的恢复要求不被安全停止抹去；官方 Core 的 Global 租约丢失/损坏专项。
- [ProxyController 测试](../crates/windows-integration/src/lib.rs)：中断发布、正常释放、补偿回无租约、连续切换保留根快照及外部修改保护。
- [recovery_tests.rs](../crates/xray-supervisor/src/recovery_tests.rs)：租约损坏仍能停止实际托管 Core。
- [app_tests.rs](../crates/xray-manager-app/src/app_tests.rs)：Reload/编辑/校验原场景、迟到选择、验证及应用上下文、历史预览、轮询合并与发送上限。两平台运行实际 GUI 状态处理代码，无需创建窗口。
- [ipc_tests.rs](../crates/xray-supervisor/src/ipc_tests.rs)：空闲/截断/慢读/不确认客户端不阻断其他查询，满额拒绝及释放后恢复，Linux fixture Core 退出时释放模拟 Global 代理。
- [Windows IPC 脚本](../scripts/windows-ipc-audit.ps1)：第二客户端在第一客户端 ACK 之前收到响应；可选真实 Core 在空闲/截断/不确认三个客户端并存时结束，验证退出检测仍工作。使用 Direct，不修改 WinINet。

Linux **64 项常规回归**通过；另有官方 Core 的租约恢复、基础运行及受控 A/B **3 项专项**通过。Linux / Windows GNU 全目标严格 clippy 与格式检查通过。

Windows GNU 构建后原生执行 **39 项常规单元回归**通过，包含 GUI 的 5 项状态测试。另有官方 Core 单元专项 **3 项**、独立 IPC **2 项**、基础 Core **1 项**、A/B 与文件占用 **2 项**通过；最后的租约检查补充后重新运行 Supervisor 16 项常规回归和租约专项，IPC 双线程测试也原生复测通过。GUI 单元可执行文件按窗口子系统编译，使用 Start-Process 显式等待并核对退出码，5 项通过。

Windows 最新 release 通过 GUI 启动/隐藏/独立退出/重开、Core/IPC、Job 强杀清理、历史成功重放与原系统代理不变。IPC 负面脚本通过；真实 Core 在空闲、截断和不确认三个客户端并存时，旧实例消失的观测耗时 **0.052 s**，并行响应不等待另一客户端 ACK。该单次观测不是性能保证。

测试初期同时进行大规模构建时暴露了 fixture 的启动/退出时序依赖及单线程慢响应序列化对测试期限的影响。混合预算回归现等待 fixture 的实际启动计数/退出，IPC 回归采用与生产更接近的双线程运行时，保留原有限时断言；最终常规回归限制为 4 个测试线程。Windows 脚本一度将等待重启的 Degraded 误判为尚未检测退出，现以实例消失或代次更新作为判据，仍保留 2 s 期限；真实 Core 日志和最终脚本均确认退出已处理。

完整日志保存在本机：

- `/tmp/xray-pilot-c13-15-linux-final.log`、`/tmp/xray-pilot-c13-15-official-linux-final.log`。
- `/tmp/xray-pilot-c13-15-windows-run.log`、`/tmp/xray-pilot-c13-15-windows-final-units.log`、`/tmp/xray-pilot-c13-15-windows-ipc-final-tests.log`。
- `/tmp/xray-pilot-c13-15-clippy-linux-final.log`、`/tmp/xray-pilot-c13-15-clippy-windows-final.log`。
- `/tmp/xray-pilot-c13-15-release-smoke.log`、`/tmp/xray-pilot-c13-15-release-ipc.log`。

可重现命令：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked -- --test-threads=4
XRAY_TEST_CORE=/path/to/xray cargo test -p xray-supervisor --lib --test real_core --test acceptance_edges --locked -- --ignored --test-threads=1
```

```powershell
$env:XRAY_TEST_CORE = 'C:\v2rayN\bin\Xray\xray.exe'
cargo test --workspace --locked -- --test-threads=4
cargo test -p xray-supervisor --lib --locked -- --ignored --test-threads=1
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\windows-ipc-audit.ps1 -BinaryDir .\target\release -CorePath $env:XRAY_TEST_CORE
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\windows-smoke.ps1 -BinaryDir .\target\release -CorePath $env:XRAY_TEST_CORE -Gui
```

Windows 使用用户提供的官方 Core v26.3.27，Linux 使用官方 v26.11.9。恢复/Global 测试使用模拟代理，release 冒烟和脚本使用 Direct；不修改真实 WinINet 或登录任务。CI 已加入新增官方 Core 租约恢复及 Windows 监督脚本，远端流水线仍无执行证据。

## 交付与仍待验收

已刷新 `dist/xray-pilot-0.1.0-windows-x64.zip` 与 `dist/SHA256SUMS`。程序包包含 3 个最新 release 可执行文件、README、LICENSE、9 份设计/实现/核对文档、2 份示例和 2 份 Windows 验证脚本，共 18 个文件。Core 仍由用户提供；压缩包校验和以 dist/SHA256SUMS 为准。

本轮不关闭 V01、V02、V04–V09 尚缺的系统和 GUI 全交互验收；V03 保持受控 A/B 范围关闭。F01 保留/压缩、F04 Core 诊断仍未完善，F02/F03 保持受限，F05 除队列与读取一致性外的交互能力继续保留。Phase 0–2 尚未全部验收。
