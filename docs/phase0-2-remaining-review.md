# Phase 0–2 剩余遗留项复核

日期：2026-11-09。范围：C01–C06 收口后的当前源码、既有回归、V01–V09 与 F01–F05。仍按 Phase 0–2 首版范围核对。

**修复前核对结论（2026-11-09）：发现 2 项 P1、1 项 P2 产品问题；V03 在受控上游范围完成，V05/V06/V07 补充通过证据。以下保留当时的触发步骤与观察，不代表当前代码仍有这些缺陷。**

**收口更新（2026-11-10）：C07–C09 已修复并加入正式回归；见 [收口记录](phase0-2-c07-09-closeout.md) 与 [当前遗留清单](phase0-2-backlog.md)。Phase 0–2 的其余验收边界仍保留。**

后续核对：C07–C09 收口后新增 C10–C12，详见 [2026-11-10 当前复核](phase0-2-current-review.md)。

## 1. C07 / P1：非运行写操作缺少可重建的结果证据

位置：[Supervisor::execute / recover](../crates/xray-supervisor/src/lib.rs)、[OperationStore](../crates/xray-supervisor/src/operations.rs)。

`CreateProfile` 原子写入正式文件，`DeleteProfile` 删除正式文件，随后统一保存 `Operation.response`。两者没有独立执行意图/结果凭据。恢复仅查询运行清单、运行 journal 和设置 journal；查询不到这些证据，就将未保存响应的请求写成 `INTERRUPTED_BEFORE_APPLY`。

定向复现步骤：

1. 使用隔离 Store 与模拟代理，执行创建或删除 Profile，记录原请求及成功结果。
2. 保持请求身份、指纹、transaction ID 与实际文件不变，仅将操作记录的 response 恢复为 null，构造“文件副作用已完成、响应尚未发布”的持久镜像。
3. 关闭并重开 Supervisor，再提交原请求，检查文件和操作结果。

| 操作 | 重启后文件 | 重建结果 | 期望 |
| --- | --- | --- | --- |
| 创建 | 已存在 | `INTERRUPTED_BEFORE_APPLY` | 重建可核验的原成功结果，不能描述为执行前中断 |
| 删除 | 已不存在 | `INTERRUPTED_BEFORE_APPLY` | 同上 |

这是真实恢复代码对构造镜像的执行结果，不代表已经实施真实断电/强杀。两个场景均复现。`ValidateDraft` / `StageHistory` 的候选持久化也没有关联结果凭据，这两种命令本轮仅静态确认，未声明完成崩溃验证。

关闭要求：所有持久写命令具备可核验的执行意图和结果关联；重启区分未执行、已提交、已补偿及恢复中；原 request ID 不重复执行，也不误报没有发生过副作用。

## 2. C08 / P1：运行期发现损坏的操作记录后仍允许新写入

位置：[Supervisor::dispatch](../crates/xray-supervisor/src/lib.rs) 中 `GetOperation` 的 get 和写请求的 lookup 入口。

操作记录的 JSON 解析错误通过 `?` 直接返回，绕过后面的阻断设置。`register` / `save` 出错会 blocked，启动扫描损坏记录也会 blocked，但运行期的这两条读取路径没有同样处理。

定向复现：创建 Profile 后，将该隔离请求记录写成截断 JSON；重复原请求，随后查询状态并用新 request ID 创建另一个 Profile。观察到：

```text
damaged request: CONFIG_PARSE_FAILED
recovery_required: false
new request: success; second Profile created
```

重复请求入口已复现；GetOperation 入口根据相同代码路径静态确认。本轮没有改写其他真实数据目录。

关闭要求：检测到损坏或矛盾的操作证据后，以明确的操作恢复错误进入 blocked，保留证据，拒绝后续普通写入；分别覆盖重复请求与 GetOperation，并验证安全清理入口。

## 3. C09 / P2：Global 对慢启动 Core 的就绪判断过早

位置：[ManagedCore::start / probe_http](../crates/xray-runtime/src/lib.rs)。

`ManagedCore::start` 等待 250 ms 后仅检查进程是否退出，随后 `probe_http` 执行一次 TCP connect。外层 3 s timeout 限制等待上界，但不会在 ConnectionRefused 时重试；探测因此可能在端点尚未绑定时提前失败。

本轮一次常规并行执行中，两个既有 Global 用例在初次应用时返回 ConnectionRefused。没有修改产品代码，重跑后 32 项常规回归通过，但这不证明时序已稳定。

进一步使用确定性 Core fixture：版本/`-test` 行为保持正常，只在实际运行时等待 700 ms 再监听。配置覆盖完整，且端口有效。观察到：

```text
listener starts after 700 ms
apply returns after approximately 300 ms
result: IO_ERROR / Connection refused
```

关闭要求：在明确期限内重试可恢复的连接失败，核对对应 Core 仍存活，并处理取消；补慢启动成功、真正超时、提前退出与失败补偿回归。Direct 尚无全部入站协议就绪检测的问题仍属于 F03。

## 4. 本轮通过的验收证据

| 范围 | 平台 / Core | 通过内容 | 证据边界 |
| --- | --- | --- | --- |
| V03 | Linux / 官方 v26.11.9；Windows / 用户提供 v26.3.27 | 通过管理器应用配置后，真实 HTTP 隧道的 example.com 及其子域收到 B 响应，other.invalid 收到 A 响应；B 返回 SOCKS 拒绝和停止监听时，A 连接记录均不增加 | 两个受控本地 SOCKS 上游，无公网账号/TLS/远端出口证明 |
| V05 / V07 | Windows / 用户提供 v26.3.27 | 文件句柄禁止原子替换正式 Profile 后，应用失败，旧正式 JSON、代次、真实旧 Core HTTP 端点及模拟代理前态保持一致 | 文件占用故障；代理使用 FakeProxy，不是 WinINet 往返 |
| V06 | Windows / 上一轮 release Supervisor | 拒绝超长帧、非法 JSON、截断帧、无请求头超时，后续正常客户端可连接；两个同时连接客户端的响应身份正确；已占用端点拒绝接管 | 同 SID、本机；未验证不同 SID、远端或高并发压力 |
| V07 | Linux / 确定性 Core fixture | connect_on_start=true 启动新代次；历史预览及重新应用往返；冻结 geo 文件缺失时停止连接并报 RESOURCE_INTEGRITY_FAILED | 不代表真实 Core 异常退出重试的全部组合已验证 |

新增正式用例：[acceptance_edges.rs](../crates/xray-supervisor/tests/acceptance_edges.rs)、[runtime_flow.rs](../crates/xray-supervisor/tests/runtime_flow.rs)。Windows IPC 验证脚本：[windows-ipc-audit.ps1](../scripts/windows-ipc-audit.ps1)。

可重现命令：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy --workspace --all-targets --locked --target x86_64-pc-windows-gnu -- -D warnings
cargo test --workspace --locked
XRAY_TEST_CORE=/path/to/xray cargo test -p xray-supervisor --test acceptance_edges --locked -- --ignored
```

```powershell
$env:XRAY_TEST_CORE = 'C:\v2rayN\bin\Xray\xray.exe'
cargo test -p xray-supervisor --test acceptance_edges --locked -- --ignored
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\windows-ipc-audit.ps1 -BinaryDir .\target\release
```

WSL 本轮交叉编译 Windows GNU 测试 exe 后在宿主 Windows 原生执行。单独运行验收用例时没有修改真实 WinINet 或计划任务；文件占用与管道占用都限于测试私有对象。CI 已加入新 Core 验收命令，但尚无远端运行结果。

C07/C08/C09 的预期失败探针源码与日志保留在本机 `/tmp/xray-pilot-remaining-audit`，已移出默认测试发现目录；对应场景现已由收口记录中的正式回归覆盖。上述步骤与观测是持久核对记录，不依赖临时探针文件长期存在。

## 5. 核对时提出的后续优先级（历史记录）

1. 修复 C07/C08 的结果恢复和写入屏障；修复 C09 的就绪等待。
2. 补 V01 真实 WinINet、V02 实际登录/权限拒绝，以及 V04–V07 剩余故障、安全与重连矩阵。
3. F01 保留/压缩策略尚未实现；F04 诊断与 F05 GUI 在途读取合并仍需完善；F02/F03 继续按明确能力范围交付。
4. 完成 V08 全交互、V09 远端 CI 与修复后的新包复测。上一轮交付包尚未包含本轮核对文档与新增测试。
