# Phase 0–2 首版实现状态

对应设计：`Xray Windows Manager架构与实现设计.md` V1.2。此文件记录已实现代码和验证边界，不替代设计中的完整验收要求。

当前状态见 [Phase 0–2 遗留项](phase0-2-backlog.md)：C01–C17 原场景保留关闭；新界面复核新增 C18（草稿保存失败仍退出）、C19（设置自动补读覆盖编辑）、C20（非当前草稿基准未落盘），均已复现、尚未修复，见 [最新复核](phase0-2-post-ui-review.md)。v0.1.1-rc.3 已发布，新界面及布局调整目前仅有本地验证包；系统验收缺口与 F01–F05 未完成部分继续保留。

## 已实现

2026-11-10 全界面长内容复核：扩展核对五个页面、六个配置标签、四个设置标签、首次设置和编辑 / 确认弹窗。修复配置主滚动正文高度约束、全局长反馈挤占工作区及 Windows 长运行名称重叠；确认弹窗的标题与操作区独立于滚动正文。逐项结果和验证边界见 [界面布局复核](ui-layout-review.md)，C18–C20 继续保留。

2026-11-10 预览长提示修复：滚动正文恢复自然高度，资源覆盖限制改为中文摘要及可展开、独立滚动详情，候选之间隔离滚动与展开状态；导入表单同步修复同类高度约束。Linux GUI 44 项、Windows GUI 46 项回归通过，详见 [长提示修复记录](ui-implementation.md#预览长提示重叠与滚动隔离2026-11-10)。现有恢复能力限制及 C18–C20 保留。

2026-11-10 编辑区域尺寸修复：配置列接收父级可用高度，JSON 使用剩余空间，结构化内容独立滚动；导入与双列预览同样自适应，短文档背景和可编辑区域填满容器。Linux GUI 43 项、Windows GUI 45 项及两平台 GUI 严格 Clippy 通过，见 [尺寸修复记录](ui-implementation.md#编辑区域随父容器自适应2026-11-10)。

2026-11-10 配置导入专项：修复 `api.tag` 内部出站误拒绝，识别反向代理 portal 路由目标；导入允许保存待补充原生 JSON 对象，概览给出字段位置与处理建议，运行前保持严格检查和官方 Core 校验。JSON 编辑器提供行号、双向滚动与格式化，导入标题和按钮固定可见，默认资源目录不被文件选择隐式替换。Windows 原生 GUI 42 项及最终 release 冒烟通过，详见 [验证记录](ui-implementation.md#配置显示原生-api-校验与待补充导入2026-11-10)。本轮未修复 C18–C20。

2026-11-10：已按用户确认的交互设计落地五页原生界面、独立持久草稿、首次设置向导及冻结运行版本诊断，并修复 Windows 路径展示及控件对齐，详见 [界面实现与验证](ui-implementation.md)。`dist/` 已提供本地验证包，新界面尚未公开发布；下文既有版本验收证据不替代新界面的验证结果。

| 范围 | 实现及入口 |
| --- | --- |
| 工程与模型 | 六个 Rust crate：`xray-model`、`xray-config`、`xray-runtime`、`windows-integration`、`xray-supervisor`、`xray-manager-app`；请求、候选、资源清单、运行代次和代理租约采用可序列化合同 |
| 原生配置 | 新建、导入、复制、删除、导出；原生 JSON 与结构化树编辑；入站/出站/路由规则顺序调整；未知字段保留；Revision 冲突检查 |
| Core | 路径选择、冻结可执行文件和 SHA-256、版本识别、固定 Core 执行 `run -test`、独立进程启停和内存日志；Windows 挂起创建、加入 kill-on-close Job 后恢复执行 |
| 配置事务 | 校验后生成不可变候选与资源清单；明确预览/应用；副作用前写 journal；正式 JSON 原子发布；`active-manifest.json` 为提交点；条件恢复正式文件、旧 Core 和代理；历史代次预览与重新校验后回滚 |
| 文件依赖 | 按原生路由及 TLS 证书字段位置枚举证书、key、geoip/geosite 和 ext 文件，复制到冻结资源集，应用及恢复前检查清单与文件哈希；来源变化不改变冻结版本；未覆盖字段/协议降低自动恢复能力 |
| 路由诊断 | 按原生顺序进行三态静态匹配；支持部分域名、IP/CIDR、端口、network、inboundTag；缺失上下文、sniffing 或不支持的前序条件返回 Unknown；实际出口固定 Unverified |
| 系统代理 | WinINet 当前用户 Global / Direct；Global 解析选定 HTTP Tag 的回环地址、数值端口和无认证要求，并验证本地 HTTP CONNECT 握手；多次切换保留首次接管快照；条件释放、读回核查及外部修改 Conflict |
| Supervisor / IPC | GUI 经 IPC 控制独立 Supervisor；Windows Named Pipe 限当前 SID/SYSTEM、拒绝远端并核查双端身份；响应接收确认后断开；数据目录单实例锁；Linux 使用 Unix socket |
| 恢复与去重 | 启动清理独立于自动连接；代次/实例/recovery epoch 栅栏；每分钟 3 次自动恢复统一预算，校验/启动失败也计入；独立请求身份与孤立证据阻断；持久 request ID 去重先于旧代次拒绝；恢复未记账提交结果；替换清单前保存原提交凭据；停止可在快照损坏时继续清理 |
| 界面与集成 | Windows 托盘打开/启停/切换模式/退出；只退出 GUI 保持 Core；独立自动连接和 Task Scheduler 登录自启设置；任务名绑定 SID/安装身份/数据目录，完整定义比对、条件补偿与重启结算；CLI 按 request ID 查询；连接失败不盲目重发写请求 |

当前不需要引入 modorch-rs：Supervisor 的单一可变状态入口已串行化命令和恢复；各模块负责自己的事务及所有权，不需要额外生命周期编排层。

## 当前实现的具体约束

- 所有旧候选、资源、Core、事务、提交凭据及操作记录都保留。尚未启用自动回收、结果压缩或容量设置，避免删除回滚和去重证据；数据量随操作增长。
- 自动依赖覆盖是明确白名单，支持常用 HTTP/SOCKS 入站及 freedom/blackhole/HTTP/SOCKS/VLESS/VMess/Trojan/Shadowsocks 出站的一部分字段和传输。DNS、复杂传输、未知嵌套字段等尚未声明完整覆盖；仍保留原生 JSON 并允许显式 Core 校验/应用，但禁止自动连接、异常重试及自动恢复旧 Core。实际提示以候选 `resources.notes` 为准。
- 文件路径必须相对于选定资源来源目录且不越界。不隐式重写绝对路径。此范围比原生 Core 更窄；用户须显式修改不兼容的路径。
- Global 验证本地 HTTP 协议就绪；Direct 当前以 Core 通过配置校验且启动后未立即退出作为就绪条件。尚未对所有原生入站执行协议探测，也不证明远端可达、服务器身份或真实出口。
- 应用 journal 记录副作用前实际存活实例，补偿不会按期望清单启动原来已停止的 Core。有关联运行/设置事务日志的未解决请求保持 `response=null` 与 `recovery_required=true`；修复后重启重建已补偿终态，历史成功结果保持稳定。结算、提交凭据或响应无法落盘时阻止后续写入。
- 创建/删除 Profile、校验候选和历史预览也保存 `runtime/transactions/<tx>/effect.json`；准备副作用与公开文件发布分离，提交点前后通过完整文件哈希/存在性判定。结果凭据保留原结果，后续文件变化不改写已完成请求；外部第三种状态保留待恢复。旧版无证据的未完成非运行写请求保守阻断并等待核对。
- 新版 Operation 记录含内容哈希，读取核对 caller、request ID、事务 ID、请求摘要与响应关联。重复请求、结果查询、历史扫描发现损坏时统一进入 OPERATION_RECOVERY_REQUIRED；请求 ID 内容冲突仍作为普通拒绝。
- 新版 v2 操作在主记录之前独立保存 caller/request ID/摘要/transaction ID 与完整性哈希。主记录或身份缺失、孤立事务/提交凭据会阻断普通写入，保留安全停止；恢复原始证据后重启再核对，不自动猜测丢失结果。旧版记录继续兼容，全部证据都被移除的历史不承诺重建。
- ListProfileSummaries 以 ID 游标返回每页至多 100 项摘要，GUI/CLI 使用摘要并按需读正文。GetOperation 的完整记录超过 1 MiB 时返回状态/错误摘要及详情哈希；GetOperationDetails 分块返回完整 JSON 字节，每块至多 1 MiB；详情变化须重新查询。其他响应超过 8 MiB 返回 IPC_MESSAGE_TOO_LARGE，写入可能已经提交，必须查询原 request ID。完整持久记录保持不变。
- 自动恢复使用同一 Supervisor 内的 60 s / 3 次尝试滑动窗口，校验和启动失败都计数。人工启停不消费或清空额度，预算耗尽后不自行恢复旧票据；Supervisor 重启不延续旧票据或单调时钟窗口。
- Global HTTP 端点使用 3 s 总就绪期限，50 ms 间隔重试可恢复连接错误，并检查 Core 是否提前退出；取消探测 future 会释放连接。Direct 的就绪限制保持不变。
- 自启设置以 `runtime/transactions/<tx>/settings.json` 记录前态/目标，以 `config/manager.json` 为设置发布点，以 `runtime/startup-owner.json` 保留预期任务定义。失败条件补偿，冲突时保留外部修改并要求恢复；成功结算记录用于响应丢失后的去重。
- 自动连接时先条件释放旧租约，再通过新事务重新获取租约；恢复动作不篡改旧操作的成功结果。
- 遇到外部代理冲突时不覆盖第三方设置。停止可完成 Core 清理并提交 Stopped，代理仍独立报告 Conflict 和错误，不报告 Released。
- 租约以独立 proxy-lease-evidence.json 记录带校验和的生命周期意图。缺失、损坏或矛盾时提供 PROXY_RECOVERY_REQUIRED，保留状态查询、安全停止及原文件，阻断普通写入；旧版 Global 提交与未结算意图也核对。正常释放/补偿回无租约可识别，仅匹配明确记录前值时自动完成中断发布。丢失或损坏证据须人工恢复可信原件后重启核对。
- GUI 读取及候选绑定选择代次、Profile、Revision、JSON 与验证上下文，迟到结果不覆盖新编辑；最多 32 个待完成请求，状态/日志在途合并。IPC 收发/确认独立于监督循环，最多 16 个活动连接与 16 项协调队列，I/O 各限 5 s；运行写入及长事务本身仍串行。
- GUI 当前为中文五页界面，支持跟随系统、浅色和深色；结构化编辑保留原生层级，任意对象字段增删及未完成数值输入仍需完善，可使用原生 JSON。托盘尚无 Profile 快捷列表；在主窗口切换配置。
- 使用独立 `xray-supervisor` 启用登录自启时，计划任务启动该独立二进制；由 GUI 启动的 Supervisor 保存的任务则启动 GUI 的后台模式。
- PAC 生成/服务/联测、TUN、Helper、订阅管理和高级统计不在本轮范围。

## 验证记录与待验收

2026-11-09 在 Linux、Rust 1.94 环境通过 19 项常规回归和 1 项真实官方 Xray v26.11.9 冒烟测试；Linux / Windows GNU 目标的严格 clippy 检查通过。两份示例均通过官方 Core 校验。仓库提供以下可重现验证：

- `cargo fmt --all -- --check` 与 `cargo clippy --workspace --all-targets --locked -- -D warnings`。
- `cargo test --workspace --locked`：原生字段及 Revision、冻结资源、未知规则、代理根快照与冲突、去重、Core 失败回滚、独立 IPC 往返、旧重试栅栏、崩溃重启预算、安全停止、提交结果重建、损坏元数据清理等回归。
- `XRAY_TEST_CORE=... cargo test -p xray-supervisor --test real_core -- --ignored`：真实官方 Core 的版本/校验、HTTP 就绪、Global 应用、幂等请求、Direct 释放、停止及重开；系统代理使用模拟后端。
- `cargo check --workspace --all-targets --target x86_64-pc-windows-gnu` 和 Windows release 交叉构建。
- CI 配置包含 Ubuntu / Windows 原生格式、lint、回归、固定官方 Core 冒烟和构建；配置已加入仓库，不能据此声称远端 CI 已执行。

本轮随后确认可通过 WSL2 互操作调用宿主 Windows x64（10.0.26300），并完成原生运行验证：

- 8 项配置、代理租约及操作去重单元测试通过；另有 Named Pipe 连续连接与跨进程去重集成测试通过。
- 使用用户指定的 `C:\v2rayN\bin\Xray\xray.exe`（v26.3.27），真实 Core 校验、Global/Direct 事务、HTTP 就绪、重复请求和停止测试通过。Global 测试使用模拟代理后端。
- 通过 Windows 原生 CLI 启动真实 Core，强杀 Supervisor 后确认 Job 自动终止其持有的 Core；重启不自动连接，并能重放已提交请求的原成功结果。
- GUI 窗口启动、关闭收起、只终止 GUI 后 Core 保持运行、重新打开 GUI 复用原 Supervisor/Core 已通过自动验证。测试前后当前用户代理的四个注册表字段一致。
- Windows 实测发现并修复 Named Pipe 实例复用导致读缓冲 EOF/数据残留的问题；每次连接使用新实例，并在关闭旧连接前创建下一监听实例。已补连续连接回归。

可使用 `scripts/windows-smoke.ps1 -BinaryDir <程序目录> -CorePath <xray.exe>` 复测；加 `-Gui` 验证窗口与关闭行为。脚本使用隔离的临时数据目录和 Direct 模式，不修改 WinINet 或登录任务。

仍需验证真实 WinINet 四项设置往返、企业 PAC、外部代理程序竞争、托盘菜单逐项交互、用户 ACL/Named Pipe 负面访问测试及 Task Scheduler 实际登录启动/权限拒绝。自动窗口测试未替代人工检查界面排版与全部交互。

恢复测试覆盖关键状态和部分持久化边界，尚未对每个写盘/副作用边界做进程强杀和断电故障注入；Windows 文件系统原子替换、磁盘满及安全软件干扰也需目标环境补测。首版不能据此标记设计 Phase 0–2 的所有验收项已通过。

## 本轮收口复测（2026-11-09）

- Linux 常规回归从 19 项增加到 **30 项**；格式检查及 Linux / Windows GNU 全目标严格 clippy 通过。
- Linux 官方 Core v26.11.9 冒烟通过，另加入未知 metadata 中 `geosite:` 文本仍可显式校验的场景。
- Windows 原生 **14 项单元回归**、Named Pipe 跨进程回归、用户指定 Core v26.3.27 冒烟通过。Core 测试使用模拟代理后端。
- Windows 原生 Task Scheduler 独立任务测试通过：当前用户启用/查询、中文与空格数据路径、外部定义修改拒绝覆盖、禁用与连续禁用。测试任务已清理，不代表已验证实际注销/登录、权限拒绝或所有入口。
- 更新后的 Windows release 程序包已复测真实 Core、IPC、Job 强杀清理及 GUI 隐藏/独立退出/重开。包内同时保留此状态说明与遗留清单。

本轮故障注入覆盖明确的持久化边界与恢复状态，不替代真实磁盘满/断电测试；V01–V09 的其余证据及 F01–F05 仍待后续完成。

## 遗留项再次核对（2026-11-09，修复前记录）

本轮新增 C07（非运行写操作响应丢失后误分类）、C08（运行期检测到操作记录损坏却未阻断新写入）两项 P1，以及 C09（Global 单次连接探测提前拒绝慢启动 Core）一项 P2。定向探针均复现问题，尚未修复产品行为；C01–C06 的修复范围不因此扩展为所有命令/故障已覆盖。

补齐的证据与回归：

- 新增 2 项常规回归：自动连接与历史预览/应用往返，冻结 geo 资源缺失时拒绝自动连接。常规回归现在共 32 项；复测全部通过，但本轮一次并行运行中两个既有 Global 用例因 ConnectionRefused 失败，随后用 700 ms 延迟监听确定复现 C09，不能据最后一次通过宣称就绪时序已稳定。
- Linux 官方 Core v26.11.9 与 Windows 用户 Core v26.3.27 的受控 A/B HTTP 隧道验证通过，含 B 的 SOCKS 拒绝和端口关闭两种故障；V03 在此范围关闭。
- Windows 原生正式文件占用测试通过，验证旧资产、代次、真实 Core 端点与模拟代理的恢复；不代表真实 WinINet 联测已完成。
- Windows 管道超长/非法/截断/无头超时、两个同时连接客户端、已占用端点拒绝接管通过；仍没有跨 SID 或远端的访问拒绝证据。
- 格式与 Linux / Windows GNU 全目标严格 clippy 通过。新增官方 Core 验收已接入 CI 配置，远端执行仍待验证。

本轮仅新增测试、验证脚本和文档记录；上一轮程序包没有包含本轮新增核对结果，后续代码修复后再更新交付包。

## C07–C09 收口（2026-11-10）

C07–C09 已关闭，复现中的失败断言已转为正式回归。Linux 常规回归 **45 项**通过，另有官方 Core v26.11.9 的基础与受控 A/B 验收 **2 项**通过；格式及 Linux / Windows GNU 严格 clippy 通过。

Windows 原生 **24 项常规单元回归**通过，另外通过真实 Core 候选/历史发布恢复 **1 项**、IPC **1 项**、基础官方 Core **1 项**、A/B 与文件占用验收 **2 项**。使用用户指定 v26.3.27，系统代理为模拟后端。慢启动连接重试与取消测试也在 Windows 原生执行。Task Scheduler 的前次证据保留，本轮未新增实际登录验证。目录锁 guard 显式解锁，补充了复制文件描述符存活时的释放/重新获取回归。

修复后的 release 程序包已更新，Windows GUI/Core/IPC/Job 冒烟和 IPC 负面脚本已复测。远端 CI 仍未执行；V01 等剩余验收与 F01–F05 均未据此关闭。

## 收口后再次核对（2026-11-10，C10–C12 修复前记录）

新增 C10 / P1、C11 / P2、C12 / P2，详见 [当前复核报告](phase0-2-current-review.md)。C10/C11 在 Linux 和 Windows 原生均复现；C12 在 Linux fixture 中约 12 s 发生 4 次自动启动尝试。产品代码尚未修改；45 项常规回归复测通过，新场景尚待转为正式修复回归。

补充 V07：Linux 官方 Core v26.11.9 和 Windows 用户 Core v26.3.27 在隔离 Direct 进程被强杀后，自动产生新代次/实例并恢复 Running；Windows 新实例的 HTTP CONNECT 握手通过。本轮未修改真实 WinINet 或注册登录任务，也未重新打包。现有 zip 仍为上一节的 C07–C09 收口版本，本轮新增复核文档尚未进入该包。


## C10–C12 收口（2026-11-10）

C10–C12 已修复并纳入正式回归。Linux 51 项常规回归通过，另有官方 Core v26.11.9 的基础与受控 A/B 两项验收通过。Linux / Windows GNU 严格 clippy 通过。独立身份丢失、孤立证据、最大配置分页/完整详情，以及混合失败的统一预算证据见 [最新收口记录](phase0-2-c10-12-closeout.md)。Windows 原生 28 项常规单元回归及 7 项专项/集成测试通过；release GUI/Core/IPC/Job、CLI 大详情及记录缺失/修复验证通过。最新程序包和校验和已刷新，详见该记录；远端 CI 仍待执行。

## C13–C15 收口（2026-11-10）

C13–C15 已关闭，详见 [收口记录](phase0-2-c13-15-closeout.md)。Linux 64 项常规回归通过，官方 Core 的租约恢复、基础运行与受控 A/B 3 项专项通过；格式及 Linux/Windows GNU 严格 clippy 通过。Windows 原生 39 项常规单元回归及 8 项专项/集成测试通过，GUI 状态测试显式等待退出；最新 release 的 GUI/Core/IPC/Job 冒烟与停滞客户端监督脚本通过。Windows 使用用户提供的 v26.3.27，恢复测试使用模拟代理，release 采用 Direct，未修改真实 WinINet 或登录任务。

独立租约凭据缺失/损坏时保守阻断并提供诊断，安全停止不会抹去恢复要求。GUI 草稿与候选绑定，32 项请求上限及轮询合并已补齐；IPC 最多 16 个连接，收发与串行运行协调分离。Windows 原子替换对短暂的 5/32/33 错误有限重试，持续文件占用仍失败并补偿。release 包、文档及校验和已刷新；V01/V02、其余故障/安全矩阵、GUI 全交互与远端 CI 仍待验收，F05 的其他编辑器/托盘能力继续保留。

## C13–C15 收口后核对与使用说明刷新（2026-11-10，C16 修复前记录）

确认 C16 的设置读取生命周期问题，产品代码尚未修复。两平台各运行原有 5 项 GUI 状态回归及 2 项新增复现/对照探针，共 7 项通过；探针通过表示确认当前错误行为。绕过 busy 界面限制的编辑覆盖探针不计为用户可触发缺陷。其余未验收和能力边界保持 [遗留清单](phase0-2-backlog.md) 的状态。

README 改为安装、配置和部署手册，技术构建及测试说明移到 [开发说明](development.md)。本地程序包同步更新文档，release 二进制保持上一轮已验证版本；新的校验和见 dist/SHA256SUMS。未新增真实系统代理、登录或远端 CI 的验收结论。

## C16 收口与 v0.1.1-rc.1 验证发布（2026-11-10）

Settings 使用独立读取代次、编辑版本与设置草稿匹配；Profile 正文加载不再受 Core 设置读取干扰。新增 5 组正式回归后，Linux 69 项常规测试、Windows 原生 44 项常规单元及 2 项独立 IPC 测试通过，GUI 两平台各 10 项。格式检查与两平台全目标严格 Clippy 通过。旧版 C16 探针保留为历史证据，不作为修复后的通过测试。

版本提升至 0.1.1-rc.1，Cargo.lock 的外部依赖保持不变。README 提供版本下载及用户验证步骤；程序包增加 VERSION.txt，并统一使用打包脚本和 SHA-256。Windows 新版 release 的 GUI/Core/IPC/Job 和停滞客户端监督脚本均通过，真实系统代理未修改。完整发布与 release 验证记录见 [C16 收口](phase0-2-c16-closeout.md)。其余 V/F 项保持清单状态，未由发布自动关闭。

## v0.1.1-rc.2：首次依赖部署与外观选择（2026-11-10）

新增 Windows x64 官方 Core/geo 下载、SHA-256 与版本验证、缓存复用、版本列表/选择和新目录修复；GUI 与 PowerShell 均有入口。安装仅写 dependencies，不修改运行代次或 manager.json；GUI 路径保存仍走既有设置事务。界面显示支持跟随系统、浅色、深色，立即应用并持久化。使用和恢复边界见 [依赖与偏好说明](installation-and-dependencies.md)。README 的正式部署不再依赖开发机 Core 路径。

v0.1.1-rc.1 的远端 [Ubuntu/Windows MSVC CI](https://github.com/muidea/xray-pilot/actions/runs/42749741396) 已全部通过；rc.2 的验证另行记录，不复用旧版本结果替代新增功能验收。

本轮 Linux **74 项**常规回归、Windows 原生 **52 项**回归通过，包含 Windows GUI 的 16 项与跨进程 IPC 两项；格式检查及两平台全目标严格 Clippy 通过。官方 v26.11.9 已通过实际联网下载和完整性校验，安装/复用、损坏检测/新目录修复、原子选择失败、排他锁及离线/错误版本/错误摘要验收通过。

Windows GNU 首次全量测试中，既有 4 MiB 配置 IPC 用例曾收到 unexpected EOF；相同二进制单独复测和随后完整 52 项复测通过，未确定首次失败原因，V06/V09 的压力与跨环境验收继续保留。

最终 Windows release 使用自动安装器下载并验证的官方 v26.11.9，通过 GUI 窗口/关闭到托盘/只退界面保持 Core/重开、Job 强杀清理、历史请求重放与启动、IPC 负面输入/停滞客户端监督冒烟。隔离数据目录分别为 `xray-pilot-smoke-2ff46ed325cf4adbab6aa91c9c0a900c`、`xray-pilot-pipe-audit-0bc5e4d7049141ec82ef7d97be5fe226`；未切换真实 WinINet 或注册登录任务。

最终 release 的实际 Windows 窗口分别以已保存的 Light/Dark 偏好启动，浅色和深色均已目视核对；三种偏好切换及保存/重读由正式回归覆盖。Settings 另提供纵向滚动及依赖按钮换行，警告使用随主题变化的配色。此项不扩展为 V08 全交互完成。

## C17 与默认数据目录（2026-11-10）

Windows 隐藏窗口后托盘菜单无法退出已复现并修复。回调和 IPC 响应主动唤醒隐藏 HWND；完整退出等待在途写操作，失败保留界面/请求身份。Windows 默认数据目录调整为安装目录下的 data，四个入口共用，显式参数仍优先，旧数据不自动迁移。Linux 77 项、Windows 原生 GUI 19 项及两平台严格 Clippy 通过；最终 release 的原生托盘/官方 Core/Job 与默认目录隔离验收通过，见 [C17 收口](phase0-2-c17-closeout.md)。

v0.1.1-rc.2 发布标签的 [Ubuntu/Windows MSVC 流水线](https://github.com/muidea/xray-pilot/actions/runs/42754591475) 已全部通过。后续复核已确认已发布 rc.3 对应 `bfe2db0` 的 [Ubuntu/Windows MSVC 流水线](https://github.com/muidea/xray-pilot/actions/runs/42761998100) 全部通过；当前未提交的新界面仍需对应源码快照的远端验收。

## 新界面与布局调整后复核（2026-11-10）

新增 C18 / P1、C19 / P2、C20 / P2，分别为草稿写盘失败后继续退出、首次设置自动补读覆盖用户编辑、非当前草稿基准更新未持久化。三个临时探针运行实际 GUI 状态代码确认错误行为后已撤回，未修复产品或发布新包。完整触发顺序与关闭条件见 [最新复核](phase0-2-post-ui-review.md)。

本轮默认并行工作区测试在启动失败补偿用例出现一次失败，单独复测通过；按 CI 的四线程配置复测，98 项常规测试通过，3 项官方 Core 专项忽略。首次失败保留为 F03 / V04 / V09 的时序不稳定待定位项，不能以最终通过关闭。新界面此前 Linux GUI 38 项、Windows GUI 40 项及原生托盘冒烟证据保持原范围，不替代新增缺陷与系统验收。
