# Xray Windows Manager — 完整架构与实现设计

**版本：** V1.2<br>
**日期：** 2026-11-09<br>
**状态：** 架构设计基线，待开发与验证<br>
**目标平台：** Windows 10 / Windows 11（首版 x64）<br>
**技术栈：** Rust + egui + windows-rs + Xray Core<br>
**设计目标：** 提供完整的 Xray 原生配置、运行及 Windows 系统集成管理能力。

V1.1 明确配置提交与崩溃恢复、Core/PAC/系统代理联合切换、连续模式切换的租约语义、统一 Direct 规则、路由模拟边界及 TUN 权限链路。本文规定设计合同，不代表相关能力已通过实现验证。

V1.2 补齐启动时失效租约清理、故障事件与重试任务的代次隔离、普通运行链的文件依赖冻结，以及跨 Supervisor 重启的 IPC 去重与操作结果恢复。

---

# 1. 项目概述

## 1.1 项目背景

Xray Core 已具备完整的代理协议、入站、出站、域名路由、DNS、流量统计和网络处理能力。

但现有 Windows 图形客户端通常使用自身的节点及配置模型，将配置转换成 Xray Core 可执行的 JSON。

这一方式存在以下问题：

1. 部分客户端无法直接使用完整的 Xray 原生配置。
2. 同时配置多个独立出站时，可能受到客户端配置模型限制。
3. 无法精确控制原生 Xray 路由规则与执行行为。
4. 配置经过转换后，可能丢失未知字段或高级配置。
5. Windows 系统代理、PAC、Core 生命周期和故障恢复之间缺少统一管理。
6. 不便于将原生 Xray 配置作为长期维护的权威配置。

因此，需要开发独立的 Windows 原生 Xray 配置管理程序。

## 1.2 项目定位

Xray Windows Manager 是基于 Rust 开发的 Windows 桌面应用。

它不重新实现代理协议，而是直接管理官方 Xray Core。

系统负责：

- Xray Core 安装与版本管理。
- 原生配置文件管理。
- 多入站、多出站管理。
- 域名与网络路由管理。
- Windows 系统代理管理。
- PAC 自动代理配置管理。
- Xray Core 运行生命周期。
- 配置验证、提交与恢复。
- Windows 登录自启与托盘管理。
- 日志、诊断与网络连通性检测。
- 后续扩展 Windows TUN、DNS、路由表和防火墙管理。

## 1.3 核心设计原则

### 原生配置优先

Xray Core 的原生 `config.json` 是 Xray 运行配置的唯一权威来源。

管理程序必须能够直接加载、编辑、验证和运行原生 JSON。

不允许强制转换成其他代理程序的配置格式。

### 执行与管理分离

Xray Core 负责：

- 代理协议。
- 网络入站。
- 网络出站。
- 路由匹配。
- DNS 处理。
- 流量转发。

Windows Manager 负责：

- 配置文件。
- 进程生命周期。
- 系统代理。
- PAC。
- GUI。
- 状态恢复。

### Windows 最小权限

桌面 GUI 默认使用普通用户权限运行。

TUN、路由表和防火墙等高权限操作通过独立的 Network Helper 执行。TUN 模式下，Helper 负责启动和约束需要提权的 Xray Core；普通 SOCKS/HTTP 模式的 Core 仍由普通用户 Supervisor 直接管理。

### 配置操作可恢复

配置修改必须遵循：

**生成候选运行代次 → 联合校验 → 保存快照与恢复日志 → 切换并验证 Core/PAC/系统代理 → 替换原生资产 → 原子提交运行清单，或恢复上一代次。**

### 不隐式切换出站

针对指定域名绑定的出站，不能因为服务器不可用就自动切换到其他出站。

---

# 2. 功能需求

## 2.1 原生配置管理

系统必须支持：

- 导入完整 Xray JSON。
- 创建和删除配置 Profile。
- 原始 JSON 编辑。
- GUI 结构化编辑。
- 配置合法性检查。
- Core 版本兼容性验证。
- 配置差异预览。
- 配置版本快照。
- 配置回滚。
- 未知字段保留。
- 原生 JSON 导出。

导入与运行校验分开：可解析的原生 JSON 对象即使不完整，也允许保留原文并保存为待补充配置；界面给出字段位置和补充建议。语法错误保留输入并要求修正，不生成猜测的节点、认证信息或直连回退。运行前必须完成静态检查、资源冻结和所选官方 Core 校验。路由目标检查须识别 `api.tag`、`reverse.portals[].tag` 等原生内部出站，不仅查询 `outbounds`。

不要求原始 JSON 经编辑后保持字节完全一致，但必须保持未修改部分的配置语义，不得丢弃未知字段。

## 2.2 入站管理

支持 Xray Core 当前版本允许的入站类型。

首版优先支持：

| 入站 | 目标 |
|---|---|
| SOCKS | 必须 |
| HTTP | 必须 |
| Mixed | 根据 Xray 版本支持情况 |
| Dokodemo-door | 原生 JSON 支持 |
| TUN | 后续阶段 |

每个入站具有独立的 Tag、监听地址、端口、协议参数和扩展配置。

默认本地入站仅绑定 `127.0.0.1`。

## 2.3 出站管理

系统允许配置任意数量的 Xray 出站。

支持通过 Xray 原生协议配置管理：

- VLESS。
- VMess。
- Trojan。
- SOCKS。
- HTTP。
- Freedom。
- Blackhole。
- 其他实际 Core 版本支持的出站。

每个出站必须使用唯一 Tag。

GUI 显示名称与实际 Tag 分离。

例如：

| 显示名称 | Tag | 用途 |
|---|---|---|
| 默认服务器 | server-a | 默认代理 |
| 专用服务器 | server-b | 指定域名代理 |
| 直连 | direct | 直接访问 |
| 阻断 | block | 拒绝访问 |

## 2.4 路由管理

支持原生 Xray RoutingObject。

首版需要支持：

- 完整域名匹配。
- 域名后缀匹配。
- IP/CIDR 匹配。
- 网络协议匹配。
- 入站 Tag 匹配。
- 出站 Tag 选择。
- 规则排序。
- 默认出口。
- 原生规则编辑。
- 路由命中模拟。

路由规则必须保持原生执行顺序。

系统不得在后台偷偷调整规则优先级。

## 2.5 Windows 系统集成

首版支持三种代理模式：

| 模式 | 说明 |
|---|---|
| Direct | 本程序不接管 Windows 用户系统代理，恢复被本程序替换的原设置 |
| Global | 将遵循系统代理的请求导向 Xray 本地 HTTP 代理 |
| PAC | 由 PAC 决定请求直连还是进入 Xray |

TUN 不作为上述三种代理模式的简单替代，而是独立的网络接管功能。

> 实现进展（2026-11-12）：PAC 基础链路已实现，包括生成 / 导入 / 编辑、有界执行测试、不可变回环发布、Windows PAC 租约目标、代次联动、端口切换、补偿和启动恢复。真实 Windows PAC URL 往返、客户端缓存差异、公网出口和完整 JScript 兼容仍按 V10 验收；本文其余 PAC 合同继续作为目标设计。

---

# 3. 总体架构

## 3.1 逻辑架构

```text
┌──────────────────────────────────────┐
│         Windows Desktop GUI          │
│              Rust + egui             │
│                                      │
│  Dashboard / Profiles / Routing      │
│  PAC / Config Editor / Diagnostics   │
└─────────────────┬────────────────────┘
                  │
                  │ IPC
                  ▼
┌──────────────────────────────────────┐
│          Runtime Supervisor          │
│                                      │
│  Configuration Transaction Manager   │
│  Xray Runtime Controller             │
│  Windows Proxy Controller            │
│  PAC Manager                         │
│  Recovery Manager                    │
│  Event & Log Manager                  │
└──────────┬───────────────┬───────────┘
           │               │
           ▼               ▼
┌──────────────────┐ ┌─────────────────┐
│    Xray Core     │ │ Windows System  │
│                  │ │                 │
│ Native Config    │ │ Proxy / PAC     │
│ Inbounds         │ │ Startup / Tray  │
│ Outbounds        │ │ Network State   │
│ Routing          │ └─────────────────┘
│ DNS              │
└────────┬─────────┘
         │
    ┌────┴─────┐
    ▼          ▼
 Server A   Server B

       可选高权限组件
┌──────────────────────────────────────┐
│       Windows Network Helper         │
│                                      │
│   TUN / Route / DNS / Firewall       │
└──────────────────────────────────────┘
```

图中 Core 为普通用户运行链；TUN 运行链通过 Supervisor 调用提权 Helper，由 Helper 启动和持有 Core，详见 §11.2 与 §15.2。

## 3.2 组件职责

### Windows GUI

负责：

- 用户交互。
- 配置展示。
- 原生 JSON 编辑。
- 节点管理。
- 路由管理。
- PAC 管理。
- 运行状态展示。
- 操作日志查看。

GUI 不直接修改系统代理，不直接管理系统路由，也不直接控制 Xray 子进程。

所有影响运行环境的操作通过 Supervisor 执行。

### Runtime Supervisor

负责：

- Xray Core 启停。
- 原生配置生效。
- 运行状态管理。
- Windows 系统代理。
- PAC HTTP 服务。
- 配置事务。
- 故障恢复。
- IPC。
- 进程监控。

Supervisor 是整个系统的唯一运行控制者。

### Xray Core

独立进程运行。

使用原生 Xray JSON 作为输入。

其网络处理行为不由 GUI 重新解释。

### Windows Network Helper

Phase 5 引入。

负责需要管理员权限的系统操作，以及 TUN Core 的受控启动、进程监控和故障清理。Supervisor 保持唯一业务控制入口，向 Helper 发出受限请求，不直接持有 TUN Core 的高权限操作能力。

不允许提供任意命令执行能力。

---

# 4. Rust 工程架构

## 4.1 Cargo Workspace

```text
xray-windows-manager/
│
├── Cargo.toml
│
├── crates/
│   │
│   ├── xray-model/
│   │   └── src/
│   │       ├── config.rs
│   │       ├── inbound.rs
│   │       ├── outbound.rs
│   │       ├── routing.rs
│   │       └── profile.rs
│   │
│   ├── xray-config/
│   │   └── src/
│   │       ├── loader.rs
│   │       ├── editor.rs
│   │       ├── validator.rs
│   │       ├── revision.rs
│   │       ├── snapshot.rs
│   │       └── transaction.rs
│   │
│   ├── xray-runtime/
│   │   └── src/
│   │       ├── process.rs
│   │       ├── job_object.rs
│   │       ├── health.rs
│   │       └── logs.rs
│   │
│   ├── pac-manager/
│   │   └── src/
│   │       ├── model.rs
│   │       ├── generator.rs
│   │       ├── importer.rs
│   │       ├── validator.rs
│   │       ├── evaluator.rs
│   │       └── publisher.rs
│   │
│   ├── windows-integration/
│   │   └── src/
│   │       ├── proxy/
│   │       ├── startup/
│   │       ├── security/
│   │       ├── network/
│   │       └── notification.rs
│   │
│   ├── xray-supervisor/
│   │   └── src/
│   │       ├── controller.rs
│   │       ├── lifecycle.rs
│   │       ├── recovery.rs
│   │       ├── ipc.rs
│   │       └── main.rs
│   │
│   ├── xray-manager-app/
│   │   └── src/
│   │       ├── main.rs
│   │       ├── app.rs
│   │       ├── tray.rs
│   │       └── views/
│   │
│   └── windows-network-helper/
│       └── src/main.rs
│
├── examples/
│   ├── dual-outbound.json
│   └── proxy.pac
│
├── tests/
│   ├── config/
│   ├── pac/
│   ├── integration/
│   └── end_to_end/
│
└── docs/
    ├── architecture.md
    ├── contracts.md
    └── acceptance.md
```

## 4.2 依赖关系

```text
xray-manager-app
       │
       ▼
Supervisor IPC Client
       │
       ▼
xray-supervisor
       │
       ├── xray-config
       ├── xray-runtime
       ├── pac-manager
       └── windows-integration
                 │
                 ▼
             xray-model
```

高权限 Helper 不应该依赖 GUI。

各模块需要通过明确接口交互。

首版暂不强制引入 Modorch-RS，优先保持轻量且可验证的 Supervisor 架构。

---

# 5. Xray 原生配置管理设计

## 5.1 Source of Truth

所有原生 Xray 配置必须直接保存为 JSON。

例如：

```text
profiles/
  default/
    config.json
  dedicated/
    config.json
```

GUI 可以对该文件执行结构化编辑。

正式资产路径为 `config/profiles/<profile_id>/config.json`。为避免校验后文件被修改，Core 可从事务目录内冻结的原生 `config.json` 启动；该文件必须与本次待提交的正式 JSON 字节相同，并记录哈希。成功提交后，正式文件、冻结副本和 Active Manifest 记录的哈希必须一致。

不得生成另一份语义不一致的隐藏配置替代用户文件。

冻结副本只用于执行已校验的版本，不是另一套配置模型。运行清单只记录版本、路径、哈希和关联资源，不替代原生 JSON。受影响 Profile 在事务期间禁止其他管理器写入；检测到外部文件变化时按 Revision 冲突处理。

## 5.2 JSON 处理策略

使用 Rust `serde_json::Value`。

要求：

1. 可以解析完整 JSON 对象。
2. 支持修改指定字段。
3. 保留未知字段。
4. 支持原始 JSON 编辑。
5. 保存时重新验证。
6. 保存前展示差异。
7. 支持撤销和恢复。

对于用户没有修改的字段，不得因 GUI 模型缺少对应属性而删除。

如需保留原始文件格式，可独立保存原始字节副本。

## 5.3 配置版本模型

配置状态定义：

```text
Draft
  │
  ▼
Parsed
  │
  ▼
StaticChecked
  │
  ▼
CoreValidated
  │
  ▼
Prepared
  │
  ▼
Applying
  │
  ├── 验证通过 ──► Committing ──► Active
  │
  └── 提交前失败 ──► Rollback ──► PreviousActive
                         │
                         └── 恢复失败 ──► RecoveryRequired
```

其中：

- Draft：当前编辑中的配置。
- Parsed：JSON 语法合法。
- StaticChecked：通过静态引用及规则检查。
- CoreValidated：通过实际 Xray Core 校验。
- Prepared：候选运行代次、旧资产快照和恢复日志均已持久化。
- Applying：正在切换 Core、PAC 和 Windows 系统代理，尚未正式提交。
- Committing：运行验证通过，正在替换正式原生资产并提交运行清单；此阶段失败仍按未提交事务恢复。
- Active：运行清单已提交，新配置已被当前 Core 加载，且通过规定的本地健康检查；接管模式还须完成系统代理读回验证。
- Rollback：新配置启用失败，恢复旧版本。
- RecoveryRequired：恢复未完成，保留日志并阻止新的运行切换，等待重试恢复或人工修复。

Committing 的失败同样进入 Rollback。提交点之后发生的故障属于已提交代次的运行故障，不得通过悄悄回写旧 Revision 来撤销提交。

必须单独记录远端代理的网络健康状态。

Active 不意味着每一个远端 Server 都可访问。

## 5.4 配置提交事务

由 Supervisor 统一执行以下流程。涉及运行环境的写操作持有全局运行切换锁，再按固定顺序获取受影响 Profile 的编辑锁；仅持有单个 Profile 编辑锁不足以切换当前 Core。

```text
1. 获取全局运行切换锁及 Profile 编辑锁
2. 检查 expectedGeneration、Profile Revision 和正式资产哈希
3. 冻结候选 JSON/PAC 及文件依赖，固定 Core 版本、资源清单及代理端点，生成 GenerationPlan
4. 解析 JSON、执行静态检查、使用目标 Core 执行 run -test，并校验 PAC
5. 保存受影响正式资产、上一运行清单及代理状态的快照
6. 持久化事务意图和 Prepared 日志；记录下一步操作后才能执行外部副作用
7. 停止旧 Core，确认其已退出；保留旧配置、Core 版本及 PAC 发布版本
8. 使用冻结的原生 config.json 启动新 Core；TUN 模式通过 Helper 执行
9. 验证进程、本地监听及必要的本地代理功能；TUN 还须验证对应数据面
10. PAC 模式发布不可变 PAC 版本并验证 URL 和脚本内容
11. 按租约规则切换系统代理端点或 PAC URL；Direct 模式执行条件恢复
12. 读回系统代理设置，核对本次 GenerationPlan；再次检查正式资产未被外部修改
13. 进入 Committing，逐一原子替换受影响的正式 config.json/proxy.pac
14. 持久化并原子替换 active-manifest.json，这是唯一正式提交点
15. 完成租约状态与结果日志，报告 Active，按保留策略清理旧资源
```

第 14 步之前任一步骤失败，都停止向前执行并按事务日志补偿已执行的操作。没有系统代理或 PAC 的阶段跳过对应副作用，但仍使用相同的提交点。候选未修改 JSON 时可复用已冻结的同一 Revision；只切换模式也必须产生新的运行代次。

IPC 写请求在第 2 步的版本检查之前，先按 §12.5 完成身份核验、持久去重查询及新请求登记。requestId 与 transactionId 的关联须先于运行副作用落盘。所有运行切换及恢复动作都通过同一协调器串行执行；进程退出事件和延迟重试不能绕过它。

上述流程描述目标 Core 状态为 Running 的应用。停止操作使用同一日志和提交点，但先条件恢复系统代理，再停止 Core 并完成 TUN 清理，保留不再被引用的 PAC 到事务结束；不执行启动新 Core 的步骤。停止代次成功返回 Stopped/Unmanaged，不报告 Active，也不删除保留的原生配置。停止事务提交前失败时，按实际副作用恢复旧代次；原状态已为 Stopped/Unmanaged 时恢复到该状态，不启动并不存在的旧 Core。

由于新旧 Core 通常需要监听相同端口，首版允许短暂的服务中断。

不承诺无中断热更新。

### 5.4.1 运行代次与联合切换

`GenerationPlan` 至少包含以下字段，并以不可变记录保存在事务目录中：

| 字段 | 含义 |
|---|---|
| transactionId / generationId / previousGenerationId | 事务、本次及上一已提交代次标识 |
| operationRef / trigger | IPC 请求记录引用或内部恢复动作标识，区分用户操作与故障恢复 |
| profileId / profileRevision / configSha256 | 原生配置身份、修订及字节哈希 |
| frozenConfigPath / assetRoot | 冻结配置绝对路径及本代次实际使用的资源根目录 |
| resourceSetId / resourceManifestSha256 / dependencyCoverage | 文件资源清单身份、哈希及枚举覆盖范围，合同见 §5.6 |
| coreVersion / corePath / coreSha256 | 固定的 Core 版本、可执行文件路径及哈希 |
| runtimeKind | UserProcess 或 ElevatedTun |
| desiredCoreState | Running 或 Stopped，独立于系统代理模式 |
| proxyMode / httpInboundTag / httpEndpoint | 系统代理目标模式及关联的 HTTP 入站 |
| pacRevision / pacSha256 / pacUrl | PAC 模式对应的不可变发布版本；非 PAC 模式为空 |
| leaseId / leaseRevision / proxyTargetDigest | 租约、本次租约修订和完整目标代理配置摘要 |

Revision 标识原生资产版本，generationId 标识一次已提交的运行组合，两者不能互换。应用配置、切换 Profile、切换代理模式和更新 Core 均通过该流程；各模块不得独立宣布整个 Profile 已生效。

计划准备期间读取代理前态并预分配 leaseId/目标修订，执行写入前再次核对前态及当前租约，不能临时换用另一个目标。desiredCoreState 为 Stopped 时系统代理目标必须为释放接管；desiredCoreState 为 Running 且 proxyMode 为 Direct 时，Core 仍可服务显式连接它的应用。

Global/PAC 模式必须显式选择一个候选配置中的 HTTP 入站 Tag，从其监听地址和端口解析 `httpEndpoint`。缺少入站、认证方式不适用于系统代理或端点不可用时，拒绝切换，不得隐式修改用户 JSON 或回退到固定的 `10809`。默认端口只用于新建配置。自定义 PAC 引用本地代理端点时，必须由用户明确声明或通过受支持分析确认关联；无法确认时要求显式修订脚本及关联信息，拒绝将其当作可自动联动的 PAC。

例如 HTTP 入站由 `10809` 改为 `20809` 时，应从同一个计划生成 Global 目标或新 PAC 内容，先验证新端点，再切换 Windows 设置。只有 Core、PAC、系统代理读回结果和正式资产均匹配计划，才可提交。PAC 脚本中没有使用本地端点的自定义逻辑必须保持原样，不能用全局字符串替换更新脚本。

### 5.4.2 持久化顺序与提交点

事务目录保存不可变候选、前态快照、GenerationPlan 和可重放的日志。所有待替换文件先在目标同一卷写入临时文件，完成写入并调用 `FlushFileBuffers`，再使用经过目标 Windows 文件系统验证的原子替换方式发布；已存在文件可使用 `ReplaceFileW`，首次创建使用对应的原子重命名方式。不得以截断后覆写正式文件的方式提交。

日志记录序号、事务身份和完整性校验，步骤意图必须写入并刷新后才能执行副作用。尾部不完整记录不能当作成功；出现有意图但没有完成记录的步骤时，通过文件哈希、进程身份或完整代理前后态核查是否已执行，再进行幂等补偿。不能仅因缺少完成标记而跳过可能已经发生的副作用。

多个 JSON/PAC/租约文件不具备跨文件原子性。`active-manifest.json` 的原子替换成功是唯一提交判据；它引用已经落盘的计划、资产哈希和租约修订。正式资产在提交点前可能已逐一替换，此时仍受事务锁保护，不能作为已提交版本返回给 GUI，也不能被另一事务读取后继续应用。Revision 随运行清单提交后才对外推进。

清单替换返回结果不明确时，必须重新读取清单并核对 transactionId、generationId 和哈希，再决定是否补偿。正式清单完整性无法确认时进入 RecoveryRequired，禁止猜测。写入最终结果日志失败不撤销已成功提交的清单；结果通过已提交代次重建。

下一次替换运行清单之前，必须把上一事务的提交凭据及关联请求终态持久化。若提交成功但结果记账失败，保留当前清单作为重建证据，允许查询和必要的安全清理，阻止新的代次提交，直到结果记录恢复。不能让后续清单覆盖唯一的提交证据。

正式 Profile 文件由用户在事务外直接编辑时，其哈希变化表示待校验修改，不能冒充当前 Active Revision。下次加载、保存或应用必须先报告差异并重新校验。校验与启动使用同一资源根目录、Core 资产环境及工作目录，冻结配置路径变化不能改变相对资源的解析语义。

### 5.4.3 中断恢复与补偿

Supervisor 启动时先恢复未完成事务，再接受运行写操作。恢复以持久化清单和事务日志为准，不能仅凭进程存活或日志最后一行判断是否提交。

| 中断位置 | 下次启动的处理 |
|---|---|
| Prepared 前 | 丢弃未发布候选，保持上一已提交代次 |
| Prepared 后、停止旧 Core 前 | 检查是否已产生副作用，撤销未提交计划，保持上一代次 |
| 新 Core/PAC 已运行、系统代理尚未切换 | 停止候选 Core，保留恢复所需 PAC，恢复并验证旧 Core |
| 系统代理已切换、清单尚未提交 | 根据租约前态/目标态核对所有权，恢复上一代次及受影响正式资产 |
| 正式资产部分或全部替换、清单仍旧 | 按资产提升凭据条件恢复本事务已替换的资产及 Revision，再恢复上一代次 |
| 清单已提交、租约或结果日志尚未完成 | 确认已提交事实并重建原操作结果；按 §5.4.4 核对实际 Core/PAC 后恢复服务或条件释放租约，不能仅将其记为 Owned |
| 清单损坏、恢复资产缺失或代理出现外部冲突 | 保留证据并进入 RecoveryRequired/Conflict，阻止自动接管，不覆盖第三方设置 |

补偿时先停止候选 Core，恢复旧正式资产及固定版本的 Core，验证旧代理端点和旧 PAC 后，才将仍由本事务拥有的 Windows 设置恢复到事务前态。若旧 Core 无法恢复且仍持有租约，尝试恢复首次接管快照并停止接管；不得让系统长期指向已知失效的本地端点。旧 Core、正式资产或代理恢复失败均记录 ROLLBACK_FAILED 并保留恢复日志，不能报告 PreviousActive。

资产提升凭据逐项记录替换前后哈希及执行状态；写入前的意图已包含前后哈希，即使没有完成标记也能核查。未被本事务替换的文件不执行恢复；已替换文件只有当前哈希仍为本次候选值时才写回前态，已经等于前态则视为恢复完成。若出现第三种哈希，保留外部修改并报告资产 Conflict，不能在补偿时覆盖它。

如发生外部代理修改，保留外部设置，独立报告代理 Conflict 与配置恢复结果。正常停止或完全退出也应先完成条件恢复，再停止被系统代理引用的 Core/PAC；无法恢复时报告失败，禁止将其记为成功释放。不能因日志清理而删除未解决的恢复记录、根快照或仍被当前系统代理引用的 PAC 版本。

持久化顺序需通过目标 Windows 文件系统的故障注入验证。突然断电后的即时网络可用性不作保证；重启后必须能确定恢复方向，不能将跨文件写入描述为原子事务。

### 5.4.4 启动恢复与连接偏好

`connect_on_start` 只控制启动时是否恢复运行服务，不控制事务核对、操作结果重建或失效代理清理。每次新 Supervisor 启动都检查未完成事务、已提交运行清单和尚未释放的租约；没有未完成事务也不能跳过租约检查。GUI 连接到仍在运行的 Supervisor 属于重连，不触发停机或重新应用启动偏好。

启动恢复在接受新的运行写操作前，按以下顺序执行：

1. 生成新的 supervisorEpoch，重建 §12.5 的操作记录及提交结果；上一个进程的定时任务和事件不直接重放。
2. 按清单判定事务是否提交，恢复未提交的正式资产；启动恢复不为补偿而绕过连接偏好，短暂启动旧 Core。
3. 核对受管运行实例、Helper 恢复状态、资源清单、PAC 发布和 Windows 完整代理设置。不能把“端口可连接”或一个 PID 当作本程序服务仍可用的证据。
4. 根据下表恢复服务或条件释放租约，写入独立恢复结果；存在资产损坏、外部冲突或恢复失败时保持可观察的阻塞状态。
5. 恢复收敛后再开放写操作；需要重新接管时通过新的运行事务完成，不凭历史清单复活已释放租约。

| 启动情况 | Core/PAC 与系统代理处理 |
|---|---|
| connect_on_start=false，原 Core/PAC 已失效 | 不启动它们；对仍匹配已记录前态/目标态的租约条件恢复根快照并标记 Released |
| connect_on_start=false，未提交事务原计划恢复旧 Core | 仅恢复旧资产及提交身份，不启动旧 Core；若系统代理仍由本租约接管，条件恢复根快照 |
| connect_on_start=true，目标为 Running 且 Frozen/Complete 资源可核验 | 验证冻结资源，恢复同一已提交配置的 Core/PAC；端点和脚本内容均就绪后才确认可继续持有尚未释放的租约 |
| 自动连接失败、资源缺失、TUN 授权被拒绝或 Helper 尚未完成清理 | 不保留已知失效代理；条件恢复根快照，分别报告运行失败与代理释放结果，不能报告 Active |
| 最新已提交目标为 Stopped | 保持停止，不因更早的 Running 记录启动或重新接管 |
| 运行目标仍为 Running，但原租约已有 Released 凭据 | 不复活旧租约；仅在允许自动连接且资源可核验时，通过新运行事务重新接管并创建新 leaseId |
| Windows 设置已被第三方修改 | 保留外部设置，标记 Conflict，不恢复根快照、不自动重新接管 |

若发现遗留 Helper/运行实例，先核验其身份并按 §15.2 完成停止或恢复交接，再决定后续动作；不能与旧 Helper 的清理并发启动新 TUN。TUN 的提权恢复未获授权时仍可执行普通用户的代理条件释放，网络状态单独保持 RecoveryRequired。

配置提交结果与启动后的实际健康状态独立：原请求已经提交则仍返回历史 Succeeded，不因本次不连接或释放代理改写为失败。历史运行清单保留已提交配置和目标；启动安全清理使用带 recoveryId、generationId、leaseId 及前后态的恢复日志，记录实际 Stopped/Unmanaged，不回退配置 Revision。该恢复记录不是新的配置提交。

恢复释放前写入并刷新意图，完成后保留租约 Released 终态凭据；若中途退出，下次从已识别的代理前态/目标态继续条件恢复。finalize 不得把已有释放凭据的租约重新置为 Owned。若需要再接管，必须建立新 leaseId 并由新代次提交。GetStatus 同时返回最后已提交代次及实际 Core/PAC/Proxy 状态，不能把“已提交”直接显示为“正在运行”。

### 5.4.5 首次部署的依赖来源

正式安装从 XTLS/Xray-core 官方版本包获取普通用户 Core 及 geo 数据，不依赖开发机或其他代理软件的安装路径。首版提供 GUI 与 PowerShell 自动安装入口，下载指定已验证版本和官方摘要，核对 SHA-256、所需条目、文件完整性与 Core 版本。安装按版本/安装 ID 并列保存，修复使用新目录，当前受管选择原子发布；缓存只有重新验证通过才能复用。下载失败、错误版本和损坏输入不能改变原选择。

Windows 默认将数据保存在程序安装目录的 data 子目录，各入口共用，且不受工作目录影响；显式数据目录参数优先。旧版数据不自动迁移或合并。

依赖安装工作不持有运行协调器的切换锁，也不直接改写 manager.json；GUI 选入路径后通过现有 SaveSettings 保存。既有 Core/资源冻结规则不变，安装、修复或选择依赖不能自动更新正在运行的代次。主题偏好支持系统、浅色和深色，由 GUI 独立保存，不构成运行切换。

## 5.5 Xray Core 命令管理

配置验证：

```powershell
xray.exe run -test -config config.json
```

配置运行：

```powershell
xray.exe run -config config.json
```

配置调试：

```powershell
xray.exe run -dump -config config.json
```

具体参数应通过受支持的 Xray Core 版本验证。

Supervisor 必须：

- 使用绝对路径调用 Core。
- 禁止拼接任意 Shell 命令。
- 捕获标准输出和错误输出。
- 记录退出码。
- 设置合理超时。
- 检查可执行文件有效性。

## 5.6 原生配置的文件依赖

普通用户 Core 与 TUN Core 均使用文件依赖清单。只冻结 JSON 和资源目录路径不足以复现配置；证书、密钥、geo 数据及其他被 Core 读取的配置资源都应按受支持 Core 版本的解析规则登记。日志输出文件、运行期 DNS 和远端 Server 的动态响应不属于不可变输入快照。

### 5.6.1 资源清单与解析语义

`ResourceManifest` 至少记录 resourceSetId、Core 版本、dependencyCoverage、清单哈希，以及逐文件的配置字段位置或资产查找键、来源规范路径、实际使用路径、大小、内容哈希、冻结位置和策略。来源路径用于追踪，验证、启动和恢复使用记录的实际路径及字节内容。复制后核对哈希，检测复制期间的源文件变化；文件缺失或不一致时不得标记资源就绪。

默认将受支持的只读文件依赖作为 Frozen 保存于独立资源集，并在运行前核对完整性。校验和运行必须使用同一资源集、工作目录和 Core 资产环境。相对路径按该字段在目标 Core 中的实际解析方式保持含义，不能一律假设相对于配置文件所在目录。

原生 JSON 仍是配置权威，资源清单不能暗中改写其字段。可通过固定工作目录、资源目录及相同相对布局选择冻结文件的，使用该布局；原生绝对路径或其他解析方式无法指向冻结内容时，展示原生路径调整的 JSON Patch，由用户确认后生成新 Revision，或明确采用下述动态依赖策略。不能将仍读取原文件的配置标记为完全冻结。

### 5.6.2 依赖策略与恢复边界

| 策略或覆盖状态 | 校验、运行与恢复合同 |
|---|---|
| Frozen / Complete | 使用已冻结字节；启动、重试和回滚先验证资源清单，原来源文件变化不影响本代次 |
| Dynamic | 用户显式选择保留原生动态引用；记录已知文件的观测哈希、时刻及适用字段，不承诺恢复旧内容 |
| CoverageIncomplete | 未知字段或扩展导致依赖无法完整枚举；保留原生字段并标注未覆盖范围，不能宣称可完整复现 |

动态文件在重新校验、启动或恢复前必须重新检查。已知内容发生变化时，不在旧代次下静默自动重启，应报告 RESOURCE_REVALIDATION_REQUIRED；经新运行事务校验并记录新的资源集后才能应用。JSON 未变时可保持 Profile Revision，但 resourceSetId 与 generationId 必须变化。运行中由 Core 原生机制重新加载动态文件时，实际行为可能变化，诊断须展示该动态依赖边界，不能把旧观测哈希当作实时内容保证。

原生模式可保留 Dynamic 或 CoverageIncomplete，但必须在应用差异和恢复能力中明确展示限制；无人值守重启和启动恢复仅对能完整核验的 Frozen 资源集自动执行，其他情况先重新校验并通过新事务应用，不能承诺 PreviousActive 已完整恢复。TUN 继续按 §15.2 拒绝无法约束或未冻结的外部输入，不因普通模式允许动态引用而放宽权限边界。

### 5.6.3 资源保留

资源集不可原地更新，更新 geo 数据或证书快照应建立新资源集和运行代次。当前运行实例、已提交清单、未解决恢复日志或保留的可恢复历史版本引用的资源不得清理。回滚使用目标版本记录的资源集，不能重新从来源目录抓取“最新”文件；若冻结副本缺失或哈希不符，报告 RESOURCE_INTEGRITY_FAILED，停止自动恢复并按 §5.4.4 条件释放失效代理。

资源文件、清单及旧版本副本使用与原生配置相同的访问控制和敏感信息保护；TUN 的受保护副本记录相同资源身份及哈希，由 Helper 再次验证。

---

# 6. 多出站及域名路由设计

## 6.1 目标

允许多个 Xray Server 同时存在。

例如：

```text
Windows Applications
          │
          ▼
    127.0.0.1:10808
          │
          ▼
       Xray Core
          │
          ▼
       Routing
          │
       ┌──┴──┐
       ▼     ▼
    Server A Server B
```

要求：

- 指定域名使用 Server B。
- 其他代理请求使用 Server A。
- 不依赖用户手动切换节点。
- 不依赖负载均衡。
- 不使用链式代理。
- Server B 不可用时不隐式切换到 A。

## 6.2 核心配置示例

下面是双出站 Xray 原生配置的示意模板。

示例假设两台服务器使用 VLESS + TLS + RAW。实际连接参数需要根据服务器协议和传输设置替换。

```json
{
  "log": {
    "loglevel": "info"
  },
  "inbounds": [
    {
      "tag": "socks-in",
      "listen": "127.0.0.1",
      "port": 10808,
      "protocol": "socks",
      "settings": {
        "auth": "noauth",
        "udp": true
      },
      "sniffing": {
        "enabled": true,
        "destOverride": [
          "http",
          "tls",
          "quic"
        ],
        "routeOnly": true
      }
    },
    {
      "tag": "http-in",
      "listen": "127.0.0.1",
      "port": 10809,
      "protocol": "http",
      "settings": {}
    }
  ],
  "outbounds": [
    {
      "tag": "server-a",
      "protocol": "vless",
      "settings": {
        "address": "server-a.example.net",
        "port": 443,
        "id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
        "encryption": "none"
      },
      "streamSettings": {
        "network": "raw",
        "security": "tls",
        "tlsSettings": {
          "serverName": "server-a.example.net"
        }
      }
    },
    {
      "tag": "server-b",
      "protocol": "vless",
      "settings": {
        "address": "server-b.example.net",
        "port": 443,
        "id": "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
        "encryption": "none"
      },
      "streamSettings": {
        "network": "raw",
        "security": "tls",
        "tlsSettings": {
          "serverName": "server-b.example.net"
        }
      }
    }
  ],
  "routing": {
    "domainStrategy": "AsIs",
    "rules": [
      {
        "type": "field",
        "domain": [
          "full:api.example.com",
          "domain:internal.example.com"
        ],
        "outboundTag": "server-b"
      },
      {
        "type": "field",
        "network": "tcp,udp",
        "outboundTag": "server-a"
      }
    ]
  }
}
```

该模板需由实际采用的 Xray Core 版本校验，不将其标记为已经完成真实网络验证的生产配置。

## 6.3 路由规则管理

GUI 应能够查看：

| 优先级 | 匹配目标 | 出站 |
|---|---|---|
| 1 | api.example.com | server-b |
| 2 | *.internal.example.com | server-b |
| 3 | 默认 TCP/UDP | server-a |

支持拖动排序。

修改排序后应显示可能影响的规则命中范围。

## 6.4 严格路由行为

配置需要保证：

- 规则明确选择对应出站。
- 不为专用出站配置未授权的自动故障切换。
- 不依赖负载均衡组默认行为。
- 规则匹配不到时明确报告默认出口。
- 支持检测目标域名是否被正确识别。

要明确：域名路由只对进入 Xray 且能够获得域名信息的请求提供确定性选择。

仅使用 IP 目标、ECH、无法识别的加密流量及绕过代理的应用，不属于普通域名规则可以完全保证的范围。

## 6.5 路由验证

静态验证：

```text
输入：api.example.com:443 / socks-in / tcp（用户明确指定）
被测配置：§6.2 示例的固定 Revision
匹配规则：1
预期出站：server-b
实际出口：Unverified
```

该示例只针对受支持且上下文足够的规则集。实际原生规则存在未知字段、缺失条件或动态依赖时，按 §18.2 返回 Unknown，不跳过前序规则继续推断。

真实验证：

```powershell
curl.exe --socks5-hostname 127.0.0.1:10808 https://api.example.com
```

测试结果需结合 Xray 日志和 Server B 的网络出口证据。

不得仅根据 HTTP 请求成功判断实际出口。

---

# 7. Windows 系统代理管理

## 7.1 设计目标

通过 Windows 系统接口管理当前用户的代理设置。

支持：

- Direct。
- Global Proxy。
- PAC。

不强制修改机器级 WinHTTP 代理。

## 7.2 Windows API

首版使用 WinINet 相关接口：

```text
InternetQueryOptionW
InternetSetOptionW

INTERNET_OPTION_PER_CONNECTION_OPTION
INTERNET_OPTION_SETTINGS_CHANGED
INTERNET_OPTION_REFRESH
```

相关配置项：

```text
INTERNET_PER_CONN_FLAGS
INTERNET_PER_CONN_PROXY_SERVER
INTERNET_PER_CONN_PROXY_BYPASS
INTERNET_PER_CONN_AUTOCONFIG_URL
```

代理类型：

```text
PROXY_TYPE_DIRECT
PROXY_TYPE_PROXY
PROXY_TYPE_AUTO_PROXY_URL
PROXY_TYPE_AUTO_DETECT
```

WinHTTP 属于独立代理配置机制。

第一阶段只读取其状态用于诊断，不自动修改机器级代理设置。

## 7.3 WindowsProxyController

建议接口：

```rust
pub trait WindowsProxyController {
    fn read_snapshot(
        &self
    ) -> Result<WindowsProxySnapshot>;

    fn apply_with_lease(
        &mut self,
        plan: &ProxyTransitionPlan
    ) -> Result<ProxyTransitionReceipt>;

    fn compensate(
        &mut self,
        receipt: &ProxyTransitionReceipt
    ) -> Result<RestoreOutcome>;

    fn finalize(
        &mut self,
        committed: &CommittedGeneration
    ) -> Result<()>;

    fn restore_if_owned(
        &mut self,
        lease_id: &LeaseId,
        expected_lease_revision: u64
    ) -> Result<RestoreOutcome>;
}
```

接口必须支持：

- 系统原配置读取。
- 原配置快照。
- 模式切换。
- 操作结果确认。
- 外部配置修改检测。
- 恢复原设置。
- 错误状态报告。

`ProxyTransitionPlan` 由运行事务构造，必须携带 transactionId、generationId、可空的现有 leaseId/expectedLeaseRevision、完整前态摘要、目标代理设置及恢复目标。控制器不得只凭一个模式枚举推断端口或新建快照。返回凭据记录此次实际执行的租约修订和前后态，供 §5.4 的提交或补偿使用。完整数据及接口合同见 §8 和 §16.4。

## 7.4 Global Proxy

新建配置的 Global 模式默认使用：

```text
HTTP Proxy:
127.0.0.1:10809
```

启用顺序：

1. 按 §5.4 创建运行计划，选择 HTTP 入站并解析本次实际代理端点。
2. 确认本次 Core 已运行，并验证选定的本地 HTTP 代理可用。
3. 读取 Windows 当前完整代理配置；有租约时核对预期状态，无租约时保存首次接管快照。
4. 持久化本次转换的前态、目标态及待应用租约修订，保留原根快照。
5. 修改 Windows 用户代理，通知设置变化。
6. 重新读取并验证完整目标设置，将转换标记为已应用但尚未提交。
7. 由运行事务完成正式资产和 Active Manifest 提交，再确认租约修订生效。

禁止在 Core 未就绪时提前启用系统代理。

Global 目标设置必须明确手动代理、bypass 和各代理类型标志；PAC、自动检测等标志按目标模式明确启停，不能残留上一模式的有效标志。快照和所有权摘要比较完整设置，不能只比较 `ProxyServer` 字符串。

## 7.5 Direct 模式

Direct 不是简单地强制关闭 Windows 的所有代理。

如果 Windows 原来存在企业 PAC、手动代理或 WPAD 配置，管理程序应在退出接管时恢复其原设置。

因此：

**Direct 的产品语义是本程序不继续接管用户系统代理。**

对于用户明确要求强制直连的情况，可以另行提供受控操作，但需要提示其影响。

模式级 `ProxyMode::Direct` 表示释放系统代理接管；§10 的域名级 `RouteAction::Direct` 表示该域名直连，两者语义独立。无租约时切换模式级 Direct 不写入 Windows 设置；有租约时只在核对所有权后恢复首次接管快照。

---

# 8. Proxy Lease 系统代理租约

## 8.1 设计目的

Windows 允许多个程序修改当前用户的代理设置。

因此需要避免：

- 覆盖其他代理软件的设置。
- 用户原始 PAC 丢失。
- 程序异常退出后无法恢复。
- 重复启动导致代理状态混乱。
- Core 已停止但系统代理仍指向本地端口。

## 8.2 租约数据模型

```json
{
  "schemaVersion": 2,
  "leaseId": "session-001",
  "owner": "xray-supervisor",
  "state": "Owned",
  "rootSnapshotRef": "snapshot-before-first-takeover",
  "leaseRevision": 3,
  "committedGenerationId": "generation-003",
  "expectedSettingsRef": "proxy-settings-003",
  "expectedSettingsDigest": "sha256:<complete-settings-digest>",
  "pendingTransition": null
}
```

实际数据结构还需要包含：

- 用户 SID。
- 会话 ID。
- 首次接管前的完整根快照，包含代理类型标志、手动代理、bypass、PAC URL 和自动检测状态。
- 当前已提交预期配置及完整摘要。
- 可空的 pendingTransition：transactionId、目标 generationId、前态/目标态引用及摘要、目标 leaseRevision、步骤和实际读回结果。
- 修改时间。
- 恢复状态。
- 错误码。
- 校验信息。
- 可空的恢复释放记录：recoveryId、原 generationId、leaseId、代理前后态及 Released 终态凭据；其优先级高于历史 finalize 记账。

`rootSnapshotRef` 在同一次接管期间不可改变。`pendingTransition` 必须先于 Windows 写入持久化，前态必须与实际读取及现有租约预期一致。租约修订只有在对应 Active Manifest 已提交后才转为已提交；提交点前已应用的状态仍由 pendingTransition 表达，不能丢失前态。

首版以 `proxy-lease-evidence.json` 独立记录带校验和的租约意图、前值摘要及发布完成窗口，先于主租约和 Windows 副作用持久化。主文件匹配已记录前值时可完成中断的发布；缺失、损坏或证据矛盾时须保留可查询的 PROXY_RECOVERY_REQUIRED，阻止普通写入，保留安全停止，不猜测根快照。旧版 Global 提交缺少租约也须阻断，且安全停止不得抹去该恢复要求；正常 Released 或补偿回无租约的持久状态须能识别。

### 8.2.1 连续切换规则

| 场景 | 快照与租约处理 |
|---|---|
| Unowned → Global/PAC | 保存当前完整设置为根快照，创建租约和待应用修订 |
| Owned Global ↔ PAC | 校验现有预期状态，复用 leaseId 和根快照，只生成新转换及租约修订 |
| Owned 下切换 Profile、端口或 Core | 保留根快照，记录事务前态和新目标态，跟随运行代次提交 |
| Owned → Direct | 条件恢复根快照；清单提交后标记 Released，提交前保留可补偿信息 |
| 未提交转换失败 | 恢复本次事务前态；不能把它误当成根快照，也不能无条件释放原租约 |
| 外部设置与预期不一致 | 标记 Conflict，拒绝自动覆盖或创建嵌套租约 |
| Released 后再次接管 | 读取当时实际设置，创建新 leaseId 和新根快照 |

企业 PAC → Global → PAC → Direct，最终必须恢复企业 PAC。重复请求不得新建租约、覆盖根快照或重复推进租约修订。

## 8.3 状态机

```text
Unowned
   │
   ▼
Preparing
   │
   ▼
IntentPersisted
   │
   ▼
Applying
   │
   ▼
Owned
   │
   ▼
Restoring
   │
   ▼
Released
```

异常状态：

```text
Conflict
RecoveryRequired
RestoreFailed
```

Owned → Applying → Owned 表示同一租约内的模式或端点转换，必须保留根快照。转换失败进入补偿；补偿成功回到原 Owned，外部修改进入 Conflict，无法恢复进入 RecoveryRequired/RestoreFailed。正常模式转换中 Restoring 到 Released 的确认受 §5.4 提交点约束；提交后的故障或启动安全清理可按 §5.4.4 的持久恢复日志完成释放，不产生新的配置提交，也不得撤销租约的 Released 终态。

## 8.4 恢复原则

恢复前必须：

1. 读取 Windows 当前代理设置。
2. 判断当前设置是否仍等于本程序写入的预期配置。
3. 如果一致，恢复原始快照。
4. 如果不一致，标记 Conflict。
5. 不盲目覆盖第三方程序的新设置。
6. 记录恢复结果。

正常释放恢复根快照；事务失败的补偿恢复事务前态。两者必须在日志中明确区分。存在未完成转换时，只能接受日志记录的完整前态或完整目标态作为可识别状态；目标态写入后未及记账的崩溃可据此恢复，其他状态一律标记 Conflict，不推测部分字段的所有权。若当前已等于根快照，正常释放可按已恢复处理。

Windows 没有提供跨代理软件的通用所有权锁。

因此租约属于应用级协调机制，不能承诺完全消除并发竞态。

## 8.5 异常恢复

| 场景 | 处理 |
|---|---|
| 正常停止代理 | 恢复代理快照 |
| Core 启动失败 | 保持系统代理原状态 |
| Core 异常退出 | Supervisor 尝试恢复 |
| GUI 崩溃 | Supervisor 继续工作 |
| Supervisor 崩溃 | 下次启动检查租约 |
| Windows 断电 | 下次登录恢复检查 |
| 外部代理修改 | 标记 Conflict |
| Global/PAC 连续切换后退出接管 | 恢复同一租约首次接管前的根快照 |
| Windows 写入后、租约确认前中断 | 对照 pendingTransition 和提交清单恢复或完成记账 |
| 首次接管失败 | 条件恢复根快照，撤销未提交租约 |
| 已有租约内切换失败 | 条件恢复事务前态，保留原租约及根快照 |
| 自动连接关闭且原端点已失效 | 不启动 Core/PAC；按恢复日志条件释放租约，配置提交结果保持不变 |
| 已有 Released 凭据却再次回放历史 finalize | 保持 Released，不重新接管；需要接管时新建租约和运行事务 |

---

# 9. PAC Manager 设计

## 9.1 PAC 定位

PAC 决定请求是否进入代理。

Xray Routing 决定进入代理的流量由哪个远程 Server 处理。

两者职责不同：

```text
Application
    │
    ▼
Windows PAC
    │
    ├── DIRECT
    │
    └── PROXY
          │
          ▼
       Xray Core
          │
          ▼
        Routing
          │
      ┌───┴───┐
      ▼       ▼
   Server A Server B
```

不得把 PAC 的代理返回值直接等同于 Xray 出站 Tag。

## 9.2 PAC 功能

必须支持：

- 自动生成 PAC。
- 导入原生 PAC。
- 原生 JavaScript 编辑。
- PAC 语法检查。
- `FindProxyForURL` 执行测试。
- PAC 文件发布。
- PAC 版本管理。
- Windows PAC URL 设置。
- PAC 服务健康检测。
- PAC 与 Xray 路由联合诊断。

## 9.3 PAC HTTP 服务

由 Supervisor 内置轻量 HTTP Server。

默认地址：

```text
http://127.0.0.1:18080/pac/<pac-revision>/proxy.pac
```

响应头：

```http
Content-Type: application/x-ns-proxy-autoconfig
Cache-Control: no-cache
```

要求：

- 仅监听 Loopback。
- 端口可配置。
- 不暴露文件系统。
- 不提供任意文件读取接口。
- 支持 PAC Revision。
- 支持不可变版本快照。
- 发布新版本时保证内容完整性。
- 支持访问日志与故障诊断。

Windows PAC URL 使用带 Revision 的不可变路径；固定 `/proxy.pac` 别名只可用于预览，不能作为事务的版本凭据。新旧发布可同时服务，旧版本在确认不再被当前系统设置引用前不得移除。端口变化属于联合切换，旧监听在条件恢复或新设置确认之前保留；无法同时提供必要版本时拒绝切换。

PAC 缓存策略只能影响缓存行为，不应承诺所有浏览器立即重新加载。

## 9.4 PAC 自动生成

示例：

```javascript
function FindProxyForURL(url, host) {
    host = host.toLowerCase();

    if (
        host === "localhost" ||
        isPlainHostName(host) ||
        dnsDomainIs(host, ".lan")
    ) {
        return "DIRECT";
    }

    if (
        host === "example.cn" ||
        dnsDomainIs(host, ".example.cn")
    ) {
        return "DIRECT";
    }

    if (
        host === "api.example.com" ||
        host === "internal.example.com" ||
        dnsDomainIs(host, ".internal.example.com")
    ) {
        return "PROXY 127.0.0.1:10809";
    }

    return "PROXY 127.0.0.1:10809";
}
```

该示例中所有需要代理的请求均交给 Xray。

Server A/B 的选择由 Xray Routing 完成。

此处展示 PAC 脚本生成形式。将其中的域名直连项用作 §10 的统一策略时，必须同时生成并确认对应 freedom 出站及 Xray 规则；不能仅把此脚本与 §6.2 的双出站模板拼接就宣称完成联合策略校验。

## 9.5 PAC 故障回退策略

默认不生成：

```javascript
return "PROXY 127.0.0.1:10809; DIRECT";
```

因为这明确允许代理失败时尝试直连。

默认生成：

```javascript
return "PROXY 127.0.0.1:10809";
```

但这并不保证所有 Windows 应用都会严格遵守 PAC。

PAC 下载失败、缓存行为和应用自己的代理实现，可能影响实际执行。

对于需要严格防泄漏的场景，必须另行实现网络层限制。

## 9.6 PAC 生命周期

```text
Disabled
    │
    ▼
Preparing
    │
    ▼
CoreReady
    │
    ▼
PACServing
    │
    ▼
WindowsApplying
    │
    ▼
CommittingGeneration
    │
    ▼
Active
```

启用 PAC 时：

1. 按 §5.4 固定 JSON、PAC、Core、HTTP 入站关联及 PAC URL，完成联合校验和恢复日志。
2. 启动或复用匹配计划的 Core，验证指定 HTTP 代理端点。
3. 启动或复用 PAC HTTP Server，发布不可变脚本版本。
4. 验证带 Revision 的 PAC URL、响应内容哈希及脚本执行结果。
5. 按 §8 创建首次租约或复用现有租约，持久化转换前态及目标态。
6. 设置 Windows PAC URL 及对应代理类型标志，通知并读取验证。
7. 由运行事务替换正式资产、提交 Active Manifest，再确认租约修订和 Active。

任意步骤失败都必须停止提交，并根据已经执行的步骤恢复相应状态。

Global/PAC 互切不得另起独立租约；PAC Manager 负责发布和探测，不能单独修改 Windows 设置或推进 Active。PAC 已提交不表示所有客户端缓存已更新，旧版引用和客户端差异按 §21.3 单独诊断。

> 实现边界（2026-11-12）：当前执行测试只提供常见 PAC 辅助函数；`dnsResolve`、`isInNet` 等需要网络/解析的辅助函数返回 Unsupported。执行环境有循环、递归、栈和脚本大小上限，但不是完整的内存取证沙箱。统一域名策略 / JSON Patch 编译器尚未实现；自动生成只做保守的域名规则编译。

---

# 10. PAC 与 Xray 路由统一管理

## 10.1 统一策略模型

GUI 可以提供统一域名策略：

```rust
pub enum RouteAction {
    Direct,
    Proxy {
        outbound_tag: String,
    },
    Block,
}

pub struct DomainRule {
    pub id: String,
    pub domain_pattern: String,
    pub action: RouteAction,
    pub priority: u32,
    pub enabled: bool,
}
```

该模型只用于受支持的规则子集。

不能取代 Xray 原生配置，也不能无损表达任意 PAC JavaScript。

## 10.2 策略编译关系

| 统一动作 | PAC 结果 | Xray 结果 |
|---|---|---|
| Direct | DIRECT | 必须路由到已确认的 freedom 直连出站 |
| Server A | PROXY | server-a |
| Server B | PROXY | server-b |
| Block | PROXY | block |

Block 需要 Xray 中存在合法的阻断出站。

PAC 本身没有通用的 BLOCK 指令。

这里的 Direct 是域名动作，必须同时产生 PAC 直连决策和 Xray 直连规则：PAC 模式下应用可直接连接；Global 模式或显式连接本地代理时，请求进入 Xray 后仍选择直连出站。直连目标按用户确认的 Tag 绑定，不能假定名为 `direct` 的出站就是 freedom；缺少合法目标时先展示新增出站的 JSON Patch，由用户确认后与规则一并提交。

统一规则按相同优先级编译到两个资产。受管理规则的插入位置、原生规则遮蔽关系及兜底规则均在差异中展示；原生规则保持相对顺序。若已有高优先级原生规则阻止预期 Direct/Proxy/Block 动作，必须报告冲突，由用户显式调整位置或改用高级模式，不能声称规则已经生效。不能无损表达的规则不进入统一编译子集。

一致性只覆盖相同域名上下文中受支持的请求：PAC 不接管的应用、域名不可识别或自定义 PAC 无法分析时，按诊断合同报告能力边界。不得通过偷偷调整原生规则优先级实现一致性。

## 10.3 原生配置保护

统一规则编辑器必须采用显式应用机制。

推荐流程：

```text
GUI 编辑规则
      │
      ▼
生成 PAC 变更
      │
      ▼
生成 Xray JSON Patch
      │
      ▼
展示配置差异
      │
      ▼
用户确认
      │
      ▼
冻结候选原生资产，不覆盖正式文件
      │
      ▼
联合校验
      │
      ▼
按 §5.4 联合切换并提交运行代次
```

对于无法表达的原生 Xray 规则，GUI 应当保留其内容并提示只读或使用高级编辑模式。

绝不允许无提示删除。

---

# 11. Runtime Supervisor

## 11.1 Supervisor 定位

Supervisor 是独立的普通用户进程。

负责管理：

- Xray Core。
- PAC HTTP Server。
- 配置事务。
- Windows 系统代理租约。
- 故障恢复。
- Runtime 日志。
- IPC。

GUI 与 Supervisor 生命周期分离。

## 11.2 进程结构

```text
xray-manager.exe
       │
       │ Named Pipe
       ▼
xray-supervisor.exe
       │
       ├── Configuration Manager
       ├── Windows Proxy Manager
       ├── PAC HTTP Server
       ├── Recovery Manager
       │
       └── xray.exe
```

上图是普通用户运行链。Phase 5 的 TUN 链路为 `Supervisor → 提权 Helper → TUN Core`：Supervisor 决定目标代次，Helper 持有高权限 Core 的进程句柄和 Job Object，并回报结构化状态。GUI 始终只调用 Supervisor。

允许 GUI 退出而 Core 继续运行。

Supervisor 需要支持 GUI 重新连接。

## 11.3 Windows 进程管理

优先使用：

```text
CreateProcessW
CreateJobObjectW
AssignProcessToJobObject
WaitForSingleObject
GetExitCodeProcess
TerminateJobObject
```

通过 Job Object 管理 Core 生命周期。

普通 Core 的 Job Object 由 Supervisor 持有，TUN Core 的 Job Object 由 Helper 持有，均启用 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`。句柄不得继承给 GUI 或 Core，避免持有者退出后仍有句柄阻止清理。子进程先挂起创建、加入 Job 后再恢复执行，避免创建与纳管之间的逃逸窗口。

GUI 不负责持有 Core 的 Job Object。

避免 GUI 退出后误杀 Core。

Job 清理只保证受管进程结束，不代表系统代理、持久路由或 DNS 已恢复。Supervisor/Helper 的正常停止必须先执行所属恢复流程；强制结束后的恢复按 §5.4.3、§8 和 §15.2 处理。

## 11.4 运行状态

```rust
pub enum CoreState {
    Stopped,
    Validating,
    Starting,
    Running,
    Degraded,
    Stopping,
    Failed,
}
```

Windows 系统代理状态应独立管理：

```rust
pub enum ProxyState {
    Unmanaged,
    Applying,
    Active,
    Restoring,
    Conflict,
    Failed,
}
```

不得把两个状态合并成简单的 Running 布尔值。

## 11.5 健康检查

至少分为：

1. Core 进程存活。
2. 本地代理端口监听。
3. 本地代理可以处理请求。
4. 远端 Server 连通性。
5. 实际路由出口验证。

GUI 必须分别展示这些状态。

Global 接管前的本地 HTTP 就绪等待必须有明确总期限；允许在期限内重试尚未监听等可恢复连接错误，不能把单次 ConnectionRefused 当作整个等待期限已耗尽。同时核对对应 Core 仍存活，进程提前退出立即交回所属事务补偿；取消探测要释放连接。首版采用 3 s 总期限与 50 ms 重试间隔。Direct 的全部原生入站探测能力另按实际实现范围声明。

远端探测失败不一定代表 Xray Core 进程异常。

## 11.6 重启策略

已提交 Core 异常退出时：

- 记录故障原因。
- 执行有限次数重启。
- 使用指数退避。
- 超过限制后停止重试。
- 在故障日志中固定 generationId、runtimeInstanceId 和事件来源；仅在 §11.6.1 的身份及代次检查仍通过时，使用该代次的完整冻结配置及资源有限重启。
- 代理端点不可用时按租约规则条件恢复根快照；不默认保留失效端点。若因此释放租约，重启成功后的重新接管必须通过新的运行事务。
- 通知 GUI。
- 保留诊断日志。

不得无限快速重启。首版同一 Supervisor 内使用 60 s 滑动窗口，最多 3 次自动恢复尝试；校验失败与实际启动失败都消费统一额度，不能为每个重试票据重新分配预算。人工启停不消费或清空自动额度。预算耗尽后停止当前票据，窗口过期只恢复额度，不自行复活已取消的任务。Supervisor 重启不沿用旧定时器及进程内单调时钟窗口。

### 11.6.1 故障事件与过期重试隔离

每次 Supervisor 启动生成新的 supervisorEpoch，每次实际 Core 启动预分配新的 runtimeInstanceId，同一 generationId 下重启也不能复用实例标识。RuntimeHandle 绑定代次、实例、监督者 epoch 及可核验的进程身份；不能只凭 PID 区分实例。Helper 的事件也必须携带相同关联信息。

所有退出、Helper 失联及重试定时事件统一进入运行协调器，在全局运行切换锁内分类，不直接调用后台重启或代理恢复：

| 事件归属 | 处理 |
|---|---|
| 正在 Applying 的候选 Core | 通知所属 transactionId，由该事务停止提交并补偿；不另起旧代次重启，也不独立释放事务中的租约 |
| 切换、停止或补偿过程中被计划结束的实例 | 按其事务步骤确认退出，不进入异常重试 |
| 当前已提交且仍是当前受管槽位的实例 | 创建故障恢复记录，按租约处理失效端点，并按策略生成有限重试任务 |
| 已被替换的实例、旧 supervisorEpoch 或已取消恢复任务 | 作为过期事件记录并丢弃，不改变 Core、代理或当前事务 |

`RestartTicket` 至少包含 generationId、失败的 runtimeInstanceId、supervisorEpoch、recoveryEpoch、重试序号和下一次执行时间。recoveryEpoch 是协调器的恢复任务版本；接受新的运行变更、停止请求或开始新的恢复尝试时使原任务失效。退避期间不持有运行切换锁。

每次定时任务执行前，重新取得全局锁并核对：监督者 epoch、当前提交代次、受管槽位记录的失败实例、recoveryEpoch、当前停止意图，以及是否存在其他 Applying/Committing 事务。任一条件不匹配即丢弃任务；不能仅因为配置文件相同或 generationId 曾经有效就继续执行。通过检查后，协调器在持锁状态下确认启动意图、创建新实例并更新槽位，避免核对与启动之间插入另一切换。

例如 g1 崩溃后退避，用户提交 g2，g1 的任务必须取消；其延迟退出事件和代理释放事件也不能修改 g2。若新事务失败后仍需要恢复 g1，由事务补偿或新的恢复记录重新生成任务，旧任务不能自行复活。后台线程、Helper 回调和 RuntimeController 都无权绕过协调器重启。

Supervisor 重启后不恢复旧定时器或继续使用旧 epoch 的任务，而是按 §5.4.4 重新核对实际状态，遵循连接偏好和资源完整性要求生成新任务。Dynamic/CoverageIncomplete 依赖不满足自动同代次重试条件时，报告待重新校验，不用最新来源文件替代冻结资源。

---

# 12. Supervisor IPC

## 12.1 通信机制

建议使用 Windows Named Pipe。

要求：

- 限制当前登录用户 SID 访问；产品不注册 Windows 服务，也不向 SYSTEM 或其他服务账户授予访问权限。
- 校验调用者身份。
- 版本化消息协议。
- 写操作串行化。
- 配置 Revision 检查。
- 结构化错误。
- 请求超时。
- 支持事件订阅。
- 运行写操作核对 expectedGeneration，避免不同 Profile 的操作绕过全局并发保护。
- requestId 绑定稳定的调用者范围及完整请求指纹，并在副作用前持久化；跨连接、GUI 与 Supervisor 重启的重试仍返回原操作状态，内容不同则拒绝复用，详见 §12.5。

连接接收、请求头/帧读取、响应发送及接收确认不得占用运行协调器。首版最多接纳 16 个连接任务，协调请求队列上限为 16，连接读/写/确认各有 5 s 期限；满额连接可以拒绝，已解析但无法入队的请求返回 IPC_BUSY。运行变更保持串行，退出检测不等待慢客户端。长事务本身仍由协调器串行执行。

GUI 异步读取须绑定选择代次与草稿版本，迟到响应不能覆盖新编辑；校验结果、历史预览和应用按钮必须与当前 Profile、Revision、JSON 摘要及 Core/资源上下文一致。首版还绑定代理模式与 HTTP Tag。Profile 正文和历史读取绑定选择及草稿，不因 Core 设置加载而被丢弃；校验和历史预览继续绑定完整验证上下文。Settings 读取使用独立请求代次及设置编辑版本，Profile 选择、加载和 JSON 编辑不能使其失效；迟到读取不能覆盖用户修改或已经提交保存的设置，旧读取也不能覆盖新读取。GUI 最多保留 32 个待完成请求，状态/日志轮询各只保留一个在途请求；未成功入队的写请求不得显示为执行中。

设置读取分别维护后台已保存基准与用户草稿；用户编辑后，读取成功仍完成首次载入并更新基准，但不覆盖草稿。正常退出须确认全部待写入草稿持久化成功，失败保留界面并提供重试、导出或明确放弃；放弃决定只适用于当时的修改。后台应用完成后，非当前配置的草稿基准也须同步持久化，写盘失败进入相同重试及退出检查。收口验证见 [C18–C20 记录](phase0-2-c18-20-closeout.md)。

Core 校验原始诊断仅进入有界会话日志，持久错误保留通用摘要。IPC 响应只序列化一次，后台编码期间继续持有连接名额，避免大响应阻塞异步工作线程或取消等待后绕过并发上限。

## 12.2 请求格式

```json
{
  "protocolVersion": 1,
  "requestId": "request-001",
  "operation": "ApplyProfile",
  "expectedGeneration": "generation-001",
  "expectedRevision": "revision-001",
  "payload": {
    "profileId": "default",
    "candidateRef": "staged-002",
    "candidateDigest": "sha256:<native-assets-and-resource-manifest>",
    "proxyMode": "Pac",
    "httpInboundTag": "http-in"
  }
}
```

## 12.3 响应格式

```json
{
  "requestId": "request-001",
  "status": "Succeeded",
  "activeGeneration": "generation-002",
  "activeRevision": "revision-002",
  "error": null
}
```

## 12.4 IPC 操作

建议定义：

```text
GetStatus
GetOperation
GetOperationDetails
ListProfiles
ListProfileSummaries
GetProfile
ValidateDraft
ApplyProfile
StartCore
StopCore
RestartCore
SetProxyMode
RestoreProxy
GetLogs
TestRoute
SubscribeEvents
```

Supervisor 不提供任意 Shell 命令执行接口。

所有写入操作必须核对调用权限和当前 Revision。

ApplyProfile、SetProxyMode、Core 更新及启停统一进入运行协调器，不允许各 IPC handler 直接调用多个模块拼接副作用。超时或 GUI 断开不代表操作被取消；客户端用 `GetOperation(requestId)` 查询结果。只有尚未产生外部副作用的操作可直接取消，之后的取消请求应转为受控补偿。

`GetStatus` 返回已提交清单标识及独立的实际 Core/PAC/Proxy 状态；未提交的新代次显示 Applying/Committing，不能作为 Active 返回。

读取接口须遵守帧预算。首版提供 ListProfileSummaries(after, limit) 的 ID 游标分页，每页最多 100 个 id/revision，正文单独读取。GetOperation 对超过 1 MiB 的完整记录返回状态摘要与详情哈希/长度；GetOperationDetails(requestId, expectedHash, offset, limit) 每次最多返回 1 MiB 原始 JSON 字节，详情变化返回 OPERATION_DETAILS_CHANGED。持久去重记录保持完整，传输摘要不等于终态压缩或 ResultDetailsExpired。8 MiB 响应超限时必须在发送任何帧字节之前改发 IPC_MESSAGE_TOO_LARGE，并保留原 request ID；响应超限不代表写事务没有提交。

## 12.5 持久去重与操作结果恢复

### 12.5.1 身份与记录

调用者范围 `callerKey` 由经过令牌核验的用户 SID 和本数据目录持久的 managerInstallationId 组成，不能使用易变的连接 ID、GUI PID 或 Supervisor PID。requestId 在该范围内唯一；客户端重连、重启和再次登录后，仍能在授权范围内查询相同请求，不能通过请求负载伪造 SID 读取他人的记录。

每个运行写请求在任何运行副作用之前，持久化一份 `OperationRecord`，至少包含：

- callerKey、requestId、requestDigest、预分配的 transactionId 和 acceptedAt。
- 规范化的完整命令：协议版本、operation、expectedGeneration、expectedRevision、payload 及候选资产内容哈希；GUI 后续修改不能改变已接受的请求。
- 状态 Accepted / Applying / Succeeded / Failed / Cancelled / RecoveryRequired，以及对应运行计划、事务日志和提交凭据引用。
- 终态结果摘要：原提交代次、原 Revision、结构化错误及实际处理结果；结果属于历史操作，不代表当前健康状态。

非运行写命令也遵守相同的去重/恢复合同。CreateProfile、DeleteProfile、ValidateDraft 和 StageHistory 必须在准备工作前持久化请求关联的执行意图；准备仅生成私有不可变副本，公开文件发布前保存目标、前态摘要与预期结果。正式 Profile 的原子写入/删除、候选 candidate.json 的发布是各自单文件提交点；不能因为这些命令不提交 active-manifest 就跳过结果证据。结果凭据必须在允许后续修改前落盘，旧结果不依赖当前文件内容重新推算。已开始准备但未发布的请求可结束为 INTERRUPTED_COMPENSATED，不能自动续做原请求；目标既不是前态也不是后态时保留 RecoveryRequired。

请求指纹使用稳定的规范化编码及哈希，覆盖所有影响执行的参数；传输连接和等待超时等不影响操作语义的字段不参与。记录采用同卷原子发布并刷新内容；transactionId 与 operationRef 在请求记录、GenerationPlan 和事务日志中相互关联。尚未登记完成的请求不得执行运行副作用。

需要应用候选资产的请求显式携带 ValidateDraft 返回的不可变 candidateRef/candidateDigest，指纹直接使用请求中的引用与摘要，不能重新读取当前 Profile 文件来计算重试身份。首次执行再核对候选内容与声明摘要；同一请求的候选不能被后续编辑替换。无需候选的启停等命令按其完整显式参数去重。

### 12.5.2 接收与去重顺序

1. 校验调用者权限、协议及消息结构，生成 callerKey 和完整请求指纹。
2. 在运行协调器内查询持久去重记录；相同 requestId 且指纹相同，直接返回原状态或原结果，不再次做 expectedGeneration/Revision 执行检查。
3. 相同 requestId 但指纹不同，返回 REQUEST_ID_CONFLICT，不覆盖原记录。
4. 新 requestId 先原子登记 Accepted 及 transactionId，再检查当前 expectedGeneration/Revision；过期请求记录为终态拒绝，不产生代理或 Core 副作用。
5. 检查通过后准备运行计划并执行事务，逐步更新操作状态；重复请求只查询此操作，不能另开执行实例。
6. 提交后持久化提交凭据与终态摘要，再返回成功。清单已提交但记账未完成时，按 §5.4.2 重建结果，不能再次执行请求。

因此，g1 → g2 的请求成功但响应丢失后，携带旧 expectedGeneration=g1 的相同请求仍应返回原 g2 结果。客户端希望基于最新状态重新执行时，必须使用新的 requestId。

### 12.5.3 Supervisor 重启与保留策略

启动时由请求记录、关联事务日志和提交清单重建操作索引，索引缓存丢失不表示请求未执行。Accepted 且没有任何执行意图或副作用的请求标记 Failed（INTERRUPTED_BEFORE_APPLY），不在重启后自动开始执行；已有执行意图的请求先恢复实际状态，再判定 Failed、RecoveryRequired 或已提交的 Succeeded。提交成功、回复前崩溃的请求必须重建为原成功结果。

下一代次提交前保存上一操作的提交凭据，确保旧 active-manifest 被替换后仍可查询原结果。内部故障和启动安全清理通过 recoveryId 关联，不修改原已提交请求的终态；GetOperation 与 GetStatus 分别展示历史操作结果及当前实际状态。

未完成或 RecoveryRequired 的操作记录及关联日志禁止清理。完整终态记录默认至少保留 30 天，之后可压缩为持久终态凭据，保留 callerKey、requestId、请求指纹、transactionId 和原终态结果摘要；该数据目录生命周期内禁止复用相同请求身份。详情已清理时返回 ResultDetailsExpired 及原终态摘要，不能当作新请求重新执行。

记录读取需核对调用者、请求/响应身份、事务身份、请求摘要与记录完整性。重复请求、GetOperation 和历史扫描检测到损坏时必须采用同一写入屏障，不能只在启动扫描或写记录失败时阻断。旧版未完成的写请求缺少执行证据时保守要求核对，不猜测为执行前中断。

登记新请求时，在主记录之前另存 caller/requestId、请求摘要、transactionId 与完整性哈希，形成独立持久身份。启动及新普通写请求登记前同时核对操作、独立身份与事务/提交凭据；操作文件缺失且仍有身份或孤立 effect/settings/运行/提交凭据时，不能作为新请求执行。能重建则返回原结果，缺少完整原请求/结果时保守阻断并保留证据，安全停止仍允许。旧记录无独立身份时仍须检查关联凭据；所有证据都被移除的历史不承诺重建，后续保留/GC 不得删除去重身份。

记录损坏或证据矛盾时报告 OPERATION_RECOVERY_REQUIRED，阻止新的运行写操作并保留证据。仅在完成日志扫描并确认不存在对应请求后，GetOperation 才返回 OperationNotFound；不能把暂时无法重建索引当作未执行。

---

# 13. Windows 桌面集成

## 13.1 GUI 技术

首版采用：

```text
Rust + egui
```

优点：

- 与 Rust 运行管理模块统一语言。
- 独立 Windows 可执行程序。
- 适合配置管理类桌面应用。
- 易于实现复杂表格、树形编辑器和状态面板。
- 后续可扩展到其他桌面平台。

需要明确，egui 属于自绘 GUI，并非 Windows 原生 WinUI 控件。

## 13.2 界面模块

Phase 0–2 界面重设计提案见 [交互设计说明](ui-interaction-design.md) 和 [可点击 Web 原型](ui-prototype/index.html)，已于 2026-11-10 确认，实现及验证见 [界面实现说明](ui-implementation.md)。方案将现阶段能力归入总览、配置、PAC、诊断、日志、设置六个主页面（PAC 页于 2026-11-12 新增）；以下模块清单继续描述完整架构范围。

建议包含：

### Dashboard

- Core 运行状态。
- 当前 Profile。
- 系统代理模式。
- PAC 服务状态。
- 最近错误。
- 当前 Core 版本。

### Profiles

- 创建 Profile。
- 导入 Profile。
- 复制 Profile。
- 删除 Profile。
- 配置快照与恢复。

### Inbounds

- 入站列表。
- 协议设置。
- 监听地址。
- 监听端口。

### Outbounds

- 出站列表。
- 节点协议。
- 安全参数。
- 传输参数。
- 出站 Tag。

### Routing

- 路由规则列表。
- 优先级调整。
- 默认出口。
- 规则命中测试。

### PAC

- PAC 模式。
- PAC 规则。
- 自定义 PAC 编辑器。
- PAC URL。
- PAC 执行测试。

### Raw JSON

- 原生 JSON 编辑。
- JSON 差异。
- 配置校验。
- 保存和应用。

### Diagnostics

- Core 诊断。
- 系统代理诊断。
- PAC 诊断。
- 路由诊断。
- 远端连通性测试。

### Logs

- Xray Core 日志。
- Supervisor 日志。
- PAC 服务日志。
- 系统集成操作日志。

## 13.3 系统托盘

支持：

- 打开窗口。
- 启动 Core。
- 停止 Core。
- 切换代理模式。
- 切换 Profile。
- 查看运行状态。
- 完全退出。

主窗口关闭默认最小化到托盘。

必须区分：

```text
关闭窗口
退出 GUI
停止 Core
停止 Supervisor
恢复系统代理
```

不能将这些操作混为一谈。

## 13.4 Windows 登录自启

首版采用 Task Scheduler。任务身份同时绑定当前 SID、持久 installation ID 与规范化数据目录，多个数据目录不能覆盖同一任务。保存预期任务定义，更新/删除前核对所有权及完整定义；外部修改进入 Conflict，保留第三方状态。禁用已经不存在且已释放的本实例任务应幂等成功。

自启设置也需要持久事务：副作用前保存任务与 manager 设置的前态/目标；任务写入后读回核查，再原子发布 manager 设置。以 manager 设置为发布点，结算结果单独持久保留；发布前失败条件补偿，重启后根据发布点决定结算成功或补偿，补偿/结算失败时阻止新的写操作。历史已成功的请求不因后续设置变化而改写结果。

两个配置项独立：

```text
start_on_logon
connect_on_start
```

前者控制是否随用户登录启动。

后者控制启动后是否自动启用上一次 Profile。

关闭 connect_on_start 不跳过启动恢复、失效租约清理或历史操作结果重建，具体顺序见 §5.4.4。自动连接只恢复能完整核验的配置和冻结资源；依赖需要重新校验时先报告原因，再通过新事务应用。

不建议默认以管理员权限运行。

---

# 14. Windows 配置存储与安全

## 14.1 文件目录

推荐：

```text
<程序安装目录>\data\
│
├── config\
│   ├── profiles\
│   │   ├── default\
│   │   │   └── config.json
│   │   └── dedicated\
│   │       └── config.json
│   └── manager.json
│
├── pac\
│   └── profiles\
│       └── default\
│           └── proxy.pac
│
├── runtime\
│   ├── active-manifest.json
│   ├── proxy-lease.json
│   ├── proxy-lease-evidence.json
│   ├── supervisor-state.json
│   ├── operations\
│   │   └── <caller-request-key>\
│   │       └── operation.json
│   ├── recovery\
│   │   └── <recovery-id>.jsonl
│   └── transactions\
│       └── <transaction-id>\
│           ├── plan.json
│           ├── journal.jsonl
│           ├── candidate\
│           │   ├── config.json
│           │   └── proxy.pac
│           └── before\
│
├── resources\
│   └── <resource-set-id>\
│       ├── manifest.json
│       └── files\
│
├── snapshots\
├── logs\
└── cache\
```

程序安装目录：

```text
%LOCALAPPDATA%\Programs\XrayManager\
```

或机器级安装：

```text
%ProgramFiles%\XrayManager\
```

配置文件不能直接依赖可执行文件所在目录。

事务中的 plan、candidate 和 before 在对应代次运行或恢复期间不可变；恢复日志在解决前不可清理。`active-manifest.json` 是提交判据，`supervisor-state.json` 只是可重建的实际运行状态。安装目录下的 Core 按版本并列保存，运行清单固定具体版本，更新不得覆盖被旧代次或恢复记录引用的文件。

operations 保存持久的请求身份、事务关联及终态凭据，索引可以重建，原始凭据不能按普通缓存删除。recovery 保存故障、启动释放及恢复结果，已释放租约的凭据不能被旧事务记账覆盖。resources 按 §5.6 保留被引用的冻结输入，不能当作下载缓存原地刷新。目录名称不得直接拼接未校验的 requestId、Profile ID 或来源路径，应使用受控标识或编码，避免越出数据根目录。

## 14.2 凭据安全

Xray 配置可能包含：

- UUID。
- Password。
- Private Key。
- 订阅凭据。
- 其他认证信息。

建议：

- 使用当前用户 ACL 保护配置目录。
- 日志输出脱敏。
- 配置导出时提示包含敏感内容。
- 不将完整配置自动写入错误日志。
- 对管理器自身独立凭据使用 DPAPI。

由于 Xray Core 需要直接读取原生 JSON，运行时配置可能必须以可读取的形式保存。

因此 DPAPI 不能直接代替原生配置文件的访问控制。

## 14.3 Core 更新

Core 更新流程：

1. 下载目标版本。
2. 验证发布来源。
3. 验证可获得的哈希或签名。
4. 保留旧版本。
5. 按 §5.6 固定现有配置的资源清单，使用新 Core 验证同一组输入；Core/geo 资源更新不能覆盖旧资源集。
6. 将新 Core 版本、路径和哈希纳入 §5.4 的 GenerationPlan。
7. 通过运行事务切换 Core、验证端点及相关 PAC/系统代理，再提交清单。
8. 提交前失败时恢复旧 Core 和全部已修改状态；提交后故障按已提交代次恢复，回退版本需另建事务。

不得在运行中直接覆盖 Core 可执行文件。

---

# 15. Windows TUN 与高级网络集成

## 15.1 目标

Phase 5 支持 Xray 原生 TUN；首版 SOCKS/HTTP 运行链不依赖 Helper。

不引入其他代理 Core 作为必需依赖。

## 15.2 高权限 Helper

新增：

```text
windows-network-helper.exe
```

负责：

- TUN 权限检查。
- 受控启动和监控提权的 TUN Core。
- 虚拟网卡相关操作。
- Windows 路由表调整。
- DNS 设置。
- 防火墙规则管理。
- 系统网络恢复。

Helper 应采用明确的操作白名单。

不允许通过 IPC 执行任意管理员命令。

### 15.2.1 权限与进程持有关系

Windows 原生 TUN 的设备打开、会话建立以及部分网络配置由 Xray Core 自身执行。预创建虚拟网卡不视为普通用户 Core 已获得 TUN 权限，不能把 Helper 仅设计为执行几条路由命令的工具。

TUN 模式采用以下固定链路：

```text
普通用户 GUI → 普通用户 Supervisor → UAC 提权 Helper → 提权 TUN Core
                                      │
                                      ├─ Core 进程句柄 / 独立 Job Object
                                      └─ 受保护配置副本 / 网络恢复日志
```

Supervisor 在停止旧 Core 前完成本次 Helper 可用性与授权检查。用户拒绝 UAC 时，保持上一已提交代次。Helper 只接受 `PrepareTun`、`StartTun`、`GetTunStatus`、`StopTun`、`RecoverTun` 等操作；请求必须绑定当前用户 SID、登录会话、Supervisor 身份、transactionId、generationId 和一次性会话凭据，Named Pipe ACL 与调用者令牌校验同时生效。不得仅以请求提供的 PID 或路径作为身份依据。

Helper 只运行已安装到受保护位置、已校验的指定 Core 与匹配架构的 Wintun DLL。不得以管理员权限执行普通用户可替换的 exe/DLL，不接受任意命令行、环境变量或 DLL 搜索目录。配置及声明的资源在启动前复制到 Helper 管理的受保护目录，核对哈希后从冻结副本运行；与普通事务一样，运行 JSON 必须保持已确认原生配置内容，不做隐式改写。

Helper 使用与 GenerationPlan 关联的 resourceSetId 和清单哈希，并逐文件验证受保护副本。普通用户侧资源与提权副本必须对应同一批已确认字节，不能在提权时重新加载来源目录的最新文件。Helper 事件携带 supervisorEpoch、generationId、runtimeInstanceId 和 transactionId，由协调器按 §11.6.1 判断归属；旧 Helper 的迟到事件不能释放新代次的代理租约。

提权配置按目标 Core 能力清单校验路径和副作用：日志等可写目标必须位于受控目录，外部资源只允许读取本次显式授权并冻结的文件，相对引用在受保护资源根目录保持相同含义。无法约束的路径、未知高权限副作用或可变外部依赖拒绝用于 TUN，并保留原配置供高级编辑；如需修改原生配置，由用户先确认差异后重新提交。Helper 用固定环境、受限工作目录和 DLL 加载策略启动 Core，防止用户目录中的内容被借提权链加载。

### 15.2.2 启停与故障恢复

1. Helper 校验版本能力、请求身份及冻结配置，在任何网络修改之前持久化网络前态、计划操作和代次标识。
2. 以挂起方式创建提权 Core，加入 Helper 独占持有的 Job Object 后恢复执行；向 Supervisor 返回不含高权限句柄的运行标识。
3. 验证 TUN 接口、路由、DNS 及实际网络状态，向 Supervisor 回报结构化结果；只有运行事务提交后才确认当前代次生效。
4. 正常停止先由 Supervisor 条件恢复依赖该 Core/PAC 的系统代理，再请求 Helper 停止。Helper 优先使用目标 Core 支持且已验证的正常退出方式，超时后终止受管 Job。
5. Core 退出后，Helper 验证原生清理结果，仅补偿仍符合本次预期且属于本次计划的网络变更，持久化恢复结果后释放资源。

每项路由、DNS、接口或防火墙变更必须在计划中确定唯一写入者：由 Core 原生能力写入，或由 Helper 写入，禁止两者重复接管。Core 自动配置的项目也要在启动前记录前态与预期范围，不能仅凭退出码宣告恢复成功。

| 故障 | 责任与恢复行为 |
|---|---|
| GUI 退出 | Supervisor 与 Helper 继续管理已提交代次 |
| Supervisor 退出或心跳超时 | Helper 独立停止 TUN Core、核查并恢复受管网络状态；用户代理由下一次普通用户恢复流程条件处理 |
| TUN Core 崩溃 | Helper 清理本次网络变更并报告；Supervisor 按租约恢复用户代理，重试仍需固定代次且次数有限 |
| Helper 正常退出 | 先完成 Core 停止及网络恢复，再关闭 Job 与进程 |
| Helper 被强制结束 | Job 关闭终止 TUN Core；不能保证所有网络设置自动恢复，下次提权先处理恢复日志 |
| 断电或恢复失败 | 保留受保护日志，下次授权启动或独立修复入口优先恢复，未解决前拒绝启动新 TUN |
| 网络项已被外部修改 | 标记网络 Conflict，不盲目覆盖，报告具体项目及手动修复信息 |

Helper 可清理网络状态，不直接代写普通用户的 WinINet 租约。Supervisor 和 Helper 的日志通过 generationId 关联；强制退出后的系统代理与网络恢复是两项独立结果。用户未再次授权时不执行需要提权的恢复，也不能将尚未执行的恢复报告为成功。

## 15.3 TUN 模式

TUN 与 PAC、Global 代理应保持明确边界。

管理器必须支持：

- TUN 启用。
- TUN 停止。
- 网络状态快照。
- 路由冲突检测。
- DNS 配置检查。
- IPv4/IPv6 检查。
- 异常恢复。
- 网卡切换检测。
- 休眠唤醒恢复。

## 15.4 避免路由回环

当 Xray Core 自身使用远端 Server 时，其出站连接不能再次被错误导入同一个 TUN 流量路径。

需要检测：

- 当前物理出口。
- 默认路由。
- TUN 路由。
- 出站接口。
- 网卡变化。

如使用 Xray 原生自动路由能力，Windows Helper 不应重复接管相同路由项；按 §15.2 的唯一写入者计划记录并验证其恢复结果。

## 15.5 防泄漏

普通 PAC 和系统代理无法保证所有程序都经过 Xray。

严格模式后续需要考虑：

- Windows Filtering Platform。
- 防火墙策略。
- DNS 限制。
- IPv6。
- 物理接口切换。
- 代理异常退出。

不能仅因为 TUN 已启动，就认为防泄漏已经完整实现。

---

# 16. 核心接口合同

## 16.1 NativeConfigStore

```rust
pub trait NativeConfigStore {
    fn load(
        &self,
        profile_id: &str
    ) -> Result<VersionedNativeConfig>;

    fn validate(
        &self,
        candidate: &StagedRevision,
        context: &ValidationContext
    ) -> Result<ValidationReport>;

    fn stage(
        &self,
        profile_id: &str,
        expected_revision: &str,
        json: &[u8],
        resources: &ResourceManifest
    ) -> Result<StagedRevision>;

    fn promote(
        &self,
        staged: &StagedRevision
    ) -> Result<PromotionReceipt>;

    fn restore(
        &self,
        receipt: &PromotionReceipt
    ) -> Result<()>;
}
```

`VersionedNativeConfig` 包含原生 JSON、Revision 与字节哈希。`ValidationContext` 固定 Core 版本、resourceSetId、资源清单哈希、覆盖范围、环境和工作目录。校验前解析并冻结依赖，Core 校验读取的必须是该资源集；不能仅验证来源路径存在。`stage` 不覆盖正式文件，返回的 StagedRevision 包含冻结 JSON 摘要及关联资源清单；ValidateDraft 将 JSON、PAC 与资源清单组合为不可变候选，返回 §12.5 的 candidateRef/candidateDigest。`promote` 只在持锁事务内原子替换正式资产，并返回可恢复凭据，不代表运行代次提交成功。PAC 正式资产采用相同的冻结、提升和恢复规则。

历史版本回滚作为一次新的运行事务读取快照、生成候选并重新校验，不能直接覆盖运行文件或倒退已提交代次。`restore` 仅用于撤销未提交事务的文件副作用。

## 16.2 RuntimeController

```rust
pub trait RuntimeController {
    fn start(
        &mut self,
        plan: &CoreLaunchPlan
    ) -> Result<RuntimeHandle>;

    fn stop(
        &mut self,
        handle: &RuntimeHandle
    ) -> Result<()>;

    fn restart(
        &mut self,
        plan: &CoreLaunchPlan
    ) -> Result<RuntimeHandle>;

    fn status(
        &self
    ) -> RuntimeStatus;
}
```

`CoreLaunchPlan` 是 GenerationPlan 的受限子集，包含代次、Core 身份、冻结配置路径与哈希、完整资源上下文、runtimeKind，以及预分配的 supervisorEpoch/runtimeInstanceId 和所属 transactionId 或 recoveryId。UserProcess 后端直接管理普通 Core；ElevatedTun 后端调用已授权 Helper，由 Helper 持有进程和 Job 句柄。RuntimeHandle 是可核验的运行标识，不能把高权限句柄暴露给 GUI。

start/stop/restart 仅由持有全局运行切换锁的协调器调用。故障恢复也须先通过 §11.6.1 的 RestartTicket 检查，后台任务不能直接调用后端。停止请求绑定具体 RuntimeHandle 及计划停止原因，迟到的退出事件按原实例处理；restart 每次创建新的 runtimeInstanceId。

## 16.3 PacService

```rust
pub trait PacService {
    fn publish(
        &mut self,
        script: &[u8],
        revision: &str
    ) -> Result<PacPublication>;

    fn probe(
        &self,
        publication: &PacPublication
    ) -> Result<PacHealth>;

    fn stop(
        &mut self,
        publication: &PacPublication
    ) -> Result<()>;
}
```

`PacPublication` 包含不可变 revision、脚本哈希和 URL。probe 核对指定版本而非固定别名；stop 只移除已确认没有系统代理或恢复记录引用的发布。发布成功不修改 Windows 设置，也不推进 Active Manifest。

## 16.4 WindowsProxyController

```rust
pub trait WindowsProxyController {
    fn read_snapshot(
        &self
    ) -> Result<WindowsProxySnapshot>;

    fn apply_with_lease(
        &mut self,
        plan: &ProxyTransitionPlan
    ) -> Result<ProxyTransitionReceipt>;

    fn compensate(
        &mut self,
        receipt: &ProxyTransitionReceipt
    ) -> Result<RestoreOutcome>;

    fn finalize(
        &mut self,
        committed: &CommittedGeneration
    ) -> Result<()>;

    fn restore_if_owned(
        &mut self,
        lease_id: &LeaseId,
        expected_lease_revision: u64
    ) -> Result<RestoreOutcome>;
}
```

apply_with_lease 持久化 pendingTransition 后修改 Windows；compensate 条件恢复事务前态；finalize 核对已提交清单及有无后续释放凭据后完成租约记账，不得将已 Released 的租约复活；restore_if_owned 用于受控释放或故障恢复，条件恢复根快照。所有副作用均须有所属事务或恢复日志，不能借恢复接口绕过代次和实例检查。首次接管、持有期间转换和释放的合同见 §8；控制器不能自行替换根快照。

## 16.5 GenerationCoordinator

```rust
pub trait GenerationCoordinator {
    fn prepare(
        &mut self,
        expected_generation: &str,
        change: RuntimeChange
    ) -> Result<GenerationPlan>;

    fn apply(
        &mut self,
        plan: GenerationPlan
    ) -> Result<CommitResult>;

    fn recover(
        &mut self,
        policy: &StartupRecoveryPolicy
    ) -> Result<RecoveryReport>;

    fn handle_runtime_event(
        &mut self,
        event: RuntimeEvent
    ) -> Result<EventOutcome>;

    fn retry(
        &mut self,
        ticket: RestartTicket
    ) -> Result<RetryOutcome>;
}
```

协调器位于 Supervisor，独占全局运行切换锁。apply 负责联合副作用、正式资产提升以及唯一的 Active Manifest 提交；其他模块无权单独提交代次。首次启动使用明确的空代次作为 expected_generation，旧运行前态为 Stopped/Unmanaged。停止操作记录新的停止目标及代理释放结果，不删除上次配置资产。RecoveryReport 分别报告配置、Core、PAC、系统代理和 TUN 网络的恢复结果。

StartupRecoveryPolicy 明确自动连接偏好、TUN 授权及资源可复现条件，不允许关闭安全清理。handle_runtime_event 负责把候选故障交给事务、确认计划退出并丢弃过期事件；retry 在锁内重新校验票据，过期时返回 Discarded，不执行重启或代理写入。prepare 接受新的运行变更时使旧恢复任务失效。RecoveryReport 和 CommitResult 中的历史提交结果与当前实际运行状态分别表达。

## 16.6 OperationStore

```rust
pub trait OperationStore {
    fn lookup(
        &self,
        caller: &CallerKey,
        request_id: &str
    ) -> Result<Option<OperationRecord>>;

    fn register(
        &mut self,
        accepted: &AcceptedOperation
    ) -> Result<OperationRecord>;

    fn finish(
        &mut self,
        transaction_id: &str,
        result: &OperationResult
    ) -> Result<()>;

    fn rebuild(
        &mut self
    ) -> Result<OperationRecoveryReport>;
}
```

该存储由 Supervisor 使用。lookup/register 在协调器中先于执行版本检查调用；register 必须原子核对同一调用者范围中的请求指纹，不能让两个连接各自创建同一请求。finish 记录提交证据和终态；rebuild 从持久记录及事务证据恢复索引。存储不直接控制 Core 或提交配置代次，具体去重及保留合同见 §12.5。

这些接口表达模块职责，尚不是可直接编译的完整 Rust 实现。

实际开发还需要补充类型定义、异步行为、错误类型和并发合同。

---

# 17. 错误处理与故障恢复

## 17.1 错误分类

`Status.last_error` 在新的运行变更（应用、代理切换、启动 / 重启、停止或完整退出）成功且操作结果持久化、后台无待恢复问题后清除。状态 / 日志读取、非运行写入与历史成功请求重放不触发清除。恢复仍被阻断时，即使安全停止返回成功也保留错误。历史失败响应继续保存在原 OperationRecord 中，GUI 日志保留已发生的失败；本地资料写入错误不因后台运行成功被清除。

| 错误 | 处理策略 |
|---|---|
| CONFIG_PARSE_FAILED | 拒绝提交 |
| CORE_VALIDATION_FAILED | 保留原配置 |
| CORE_START_FAILED | 回滚 |
| GENERATION_CONFLICT | 拒绝过期运行切换，返回当前已提交代次 |
| ASSET_REVISION_CONFLICT | 保留外部修改，拒绝覆盖；已发生副作用则进入受控补偿 |
| PROXY_ENDPOINT_INVALID | 缺少可用 HTTP 入站或 PAC 关联，拒绝接管 |
| GENERATION_COMMIT_FAILED | 核对提交清单，确认未提交后补偿 |
| RECOVERY_REQUIRED | 保留恢复记录，阻止新的运行写操作 |
| RESOURCE_INTEGRITY_FAILED | 冻结输入缺失或哈希不符，停止自动恢复并条件释放失效代理 |
| RESOURCE_REVALIDATION_REQUIRED | 动态依赖变化或覆盖不完整，不在旧代次下自动重启，重新校验并通过新事务应用 |
| REQUEST_ID_CONFLICT | 同一请求身份对应不同指纹，拒绝执行且不覆盖原记录 |
| INTERRUPTED_BEFORE_APPLY | 已登记但尚无执行意图的请求被中断，保存终态，不自动重放 |
| INTERRUPTED_COMPENSATED | 已有执行意图，恢复已完成补偿且没有提交，保存失败终态，不自动重放 |
| STARTUP_CONFLICT / SETTINGS_CONFLICT | 保留外部任务/设置，已有副作用则进入条件恢复 |
| STARTUP_FAILED | 计划任务调用或读回失败，补偿并保留故障证据 |
| OPERATION_RECOVERY_REQUIRED | 操作记录损坏或证据矛盾，阻止新的运行写操作，保留恢复证据 |
| PORT_CONFLICT | 报告端口冲突 |
| PAC_SCRIPT_INVALID | 拒绝启用 PAC |
| PAC_ENDPOINT_UNAVAILABLE | 不修改 Windows PAC |
| PROXY_POLICY_DENIED | 报告系统策略限制 |
| PROXY_LEASE_CONFLICT | 不覆盖外部设置 |
| PROXY_RECOVERY_REQUIRED | 租约或独立生命周期凭据缺失、损坏或矛盾；保留查询/安全停止，阻止普通写入，先修复原始证据 |
| CORE_CRASHED | 有限重启 |
| ROLLBACK_FAILED | 严重告警，保留日志并进入 RecoveryRequired |
| NETWORK_ROUTE_CONFLICT | 网络恢复流程 |
| TUN_AUTHORIZATION_DENIED | 不启动提权 Core，保持上一代次 |
| TUN_PRIVILEGED_CONFIG_REJECTED | 拒绝不受控路径或副作用，保留原生配置供修订 |

## 17.2 恢复优先级

优先保证：

1. 系统不会保留本程序错误写入的代理状态。
2. 当前有效配置不会被非法配置覆盖。
3. Core 不会产生无限重启循环。
4. 用户原先的 PAC 或手动代理不会被无条件清除。
5. 失败状态必须可观察。

## 17.3 故障恢复限制

以下情况不能承诺即时恢复：

- Windows 突然断电。
- Supervisor 被强制结束。
- 用户强制修改网络设置。
- 组织策略覆盖代理。
- 系统账户权限变化。
- 其他代理软件并发修改。

对于这些情况，提供下次启动恢复检查和手动修复入口。

---

# 18. 诊断体系

## 18.1 诊断层级

```text
ConfigParsed
     │
     ▼
CoreValidated
     │
     ▼
CoreStarted
     │
     ▼
LocalProxyReady
     │
     ▼
PACReady（PAC 模式，含内容哈希）
     │
     ▼
WindowsProxyApplied（接管模式）
     │
     ▼
GenerationCommitted
     │
     ▼
RouteMatched
     │
     ▼
RemoteOutboundConfirmed
```

每个状态单独报告。

图中顺序是联合切换的验证依赖，不表示 RouteMatched 或 RemoteOutboundConfirmed 是 Active 的前提；远端网络状态单独展示。Direct 模式跳过系统接管验证；纯 TUN 配置以 TunDataPlaneReady 替代 LocalProxyReady，并独立报告网络恢复状态。

## 18.2 路由测试

路由测试分为静态模拟和真实运行观测。仅输入 URL 是诊断草稿，不保证足以确定原生路由；GUI 允许补充上下文，并逐项显示哪些值来自用户、计划、探测或假设。

### 18.2.1 输入上下文

| 输入 | 用途与缺失处理 |
|---|---|
| generationId / configSha256 / coreVersion | 固定被测配置及 Core；草稿单独标记，不伪装成 Active |
| resourceSetId / resourceManifestSha256 / dependencyCoverage | 固定证书、geo 等文件输入；动态或未覆盖部分不得声称已冻结 |
| url / host / targetPort | PAC 与域名规则；URL 的规范化及默认端口需展示 |
| inboundTag / network | 确定入站限制及 TCP/UDP 规则；不能从 HTTPS URL 静默假定 TCP |
| sourceIp / sourcePort / targetIps | 来源与 IP/CIDR 规则；未知时按三值逻辑传播 |
| originalDestination / sniffedDomain / detectedProtocol | 区分原始目标与嗅探结果，不把 URL 主机当成已观测到的嗅探域名 |
| user / process 等规则属性 | 遇到对应规则时必须提供，或返回 Unknown |
| DNS 结果、解析时刻、domainStrategy、geo 数据版本 | 仅在实现支持且资源固定时用于模拟；无法复现则返回 Unknown |
| PAC revision、执行引擎、DNS/时间上下文 | 固定 PAC 测试条件；与实际客户端环境的差异必须展示 |

原生规则按顺序使用 Match / NoMatch / Unknown 判断，并保持字段组合语义。只有此前规则均能确定为 NoMatch，当前规则能确定为 Match，才返回确定的静态命中。更早规则遇到不支持字段或缺失上下文且不能确定不匹配时，必须停止确定性推导；不能跳过它继续声称命中后续兜底规则。默认出站也须从本次原生配置推导，无法确定时不能默认使用 server-a。

模拟器维护按 Core 版本声明的能力清单。未实现的原生匹配语义、无法固定的 DNS/geo 资源、动态 balancer 或不可推断的嗅探行为均返回 Unknown；不能为完成首版验收而简化成字符串匹配器。

### 18.2.2 输出与证据

结果至少包含 `kind`（StaticSimulation / RuntimeObservation）、被测代次、上下文来源、PAC 结果、Xray 命中状态、阻塞判断的规则/字段、候选或确定的 outboundTag，以及证据引用。静态结果使用“预期出站”，不得显示为“已验证出口”。

上下文完整且匹配能力受支持时的输出示例：

```text
Kind: StaticSimulation
Generation: generation-003
Context: http-in / tcp / api.example.com:443（用户明确指定）
PAC: PROXY 127.0.0.1:20809（固定 PAC revision 的模拟结果）
Xray Match: MatchKnown / Rule 1
Expected Outbound: server-b
Actual Outbound: Unverified
```

上下文不足或前序规则无法模拟时的输出示例：

```text
Kind: StaticSimulation
Xray Match: Unknown
Reason: Rule 1 依赖 inboundTag，当前输入未提供
Expected Outbound: Unknown
Actual Outbound: Unverified
```

对于自定义 PAC，必须通过隔离且有执行时间、内存和 DNS 调用限制的 PAC 执行环境评估脚本，提供标准辅助函数；禁止任意文件、进程和网络访问。未实现的辅助函数、超时或运行错误分别报告 Unsupported/Error，不得静默返回 DIRECT。该结果仅描述指定执行环境，不能替代真实 Windows 客户端行为验证。

RuntimeObservation 需绑定同一代次、实际入站与测试请求，并附 Core 路由日志或可用的实际匹配观测、远端出口证据；证据不足时保留 Unverified。静态模拟、PAC 执行和真实出口是独立结果。

## 18.3 真实网络检测

可以使用：

```powershell
curl.exe --socks5-hostname 127.0.0.1:10808 https://api.ipify.org
```

该命令只能验证此次请求到检测站点的出口，不能证明 `api.example.com` 的原生规则命中。指定域名测试必须针对实际目标、关联的 Core 路由证据和远端证据作出结论。

如用户选择临时规则进行出站探测，规则变更和测试后的恢复均通过运行事务执行，保存原 Revision 并报告诊断代次；该结果仅证明指定出站可用。测试中断后仍须恢复原规则，不能绕过 Revision 检查直接写回文件，也不能将修改后路由的结果当作原路由的验证。

## 18.4 日志管理

日志类型：

- Core Runtime。
- Supervisor。
- Config Transaction。
- PAC。
- Windows Proxy。
- Diagnostics。

应支持：

- 日志级别。
- 日志大小限制。
- 保留周期。
- 敏感信息脱敏。
- 时间戳。
- 请求关联 ID。
- 配置 Revision。
- transactionId、generationId、runtimeInstanceId、supervisorEpoch/recoveryEpoch 及 resourceSetId。
- 操作提交结果与启动安全清理的 recoveryId，分别记录历史结果和当前健康状态。

---

# 19. 开发阶段规划

## Phase 0：基础工程与 Xray Core 运行

### 实现内容

- Cargo Workspace。
- 基础 egui 窗口。
- Core 路径配置。
- Core 版本检测。
- 原生 JSON 加载。
- Core 配置校验。
- Core 启停。
- 独立 Supervisor 的最小进程、Named Pipe 启动连接及普通 Core Job 管理。
- 运行日志。

### 验收要求

- 能直接加载完整 Xray 原生配置。
- 错误配置不能启动。
- 正确记录退出码。
- 不发生 Shell 命令注入。
- 能显示 Core 实际运行状态。
- GUI 通过 Supervisor 控制 Core，后续阶段无需迁移进程所有权。

---

## Phase 1：多出站与配置事务

### 实现内容

- 多入站。
- 多出站。
- 域名路由。
- 出站 Tag 管理。
- JSON 编辑器。
- Revision。
- 快照。
- 配置事务。
- 回滚。
- GenerationPlan、事务日志和 Active Manifest 提交点；暂不接管系统代理时跳过相应副作用。
- 普通运行链的文件依赖枚举、冻结资源清单、完整性验证与保留策略。
- 声明能力范围和上下文的静态路由测试。

### 验收要求

- Server A/B 可同时配置。
- 指定域名固定访问 Server B。
- 其他代理请求使用 Server A。
- Server B 故障不自动切换到 A。
- 配置错误不影响旧版本。
- 未知原生字段得到保留。
- 成功应用后，正式 JSON、冻结副本、Revision 和清单哈希一致；重启读取相同的已提交版本。
- 在各持久化和副作用边界注入退出，提交点前恢复旧版本，提交点后识别新版本，不出现混合状态。
- 路由输入缺失或前序规则不受支持时返回 Unknown，不把静态预期展示为真实出口。
- 原来源目录中的证书或 geo 文件变化不影响已冻结代次；冻结资源缺失时明确停止自动恢复，不能改读最新来源文件。
- 候选故障交回所属事务，计划停止不触发后台重试；动态和未覆盖依赖的恢复限制可见。

---

## Phase 2：Windows 系统代理与 Supervisor 恢复

### 实现内容

- 完善已有 Supervisor、Named Pipe IPC 与 Job Object 的重连、事件及恢复能力。
- Windows WinINet 管理。
- Proxy Lease。
- 托盘。
- 登录自启。
- 异常退出恢复。
- 独立于自动连接偏好的启动安全清理，及代次/实例/epoch 约束的重试任务。
- 持久 OperationRecord、requestId 去重、提交凭据与跨重启结果索引重建。

### 验收要求

- GUI 退出不影响 Core。
- GUI 重新启动可以连接原 Supervisor。
- 原有系统代理可恢复。
- 企业 PAC 不被破坏。
- 外部代理修改可以检测。
- Core 异常退出能够处理。
- 不重复启动 Core。
- 修改 HTTP 入站端口时联动 Global 目标；切换失败同时恢复旧 Core 和代理状态。
- 首次接管快照不被 Profile/端口切换覆盖；Direct 释放恢复原始设置。
- IPC 超时重试不重复提交代次或创建租约，未完成操作可按 requestId 查询。
- connect_on_start=false 时不启动旧 Core，但仍条件恢复失效代理；原已提交操作仍能查询到成功结果。
- g1 退避期间提交 g2 后，g1 的重试和迟到事件不改变新 Core 或租约。
- 提交成功但响应丢失后重启 Supervisor，旧 expectedGeneration 的同一请求返回原成功结果。
- 操作结果压缩后保留终态摘要，旧 requestId 不会作为新命令再次执行。

---

## Phase 3：PAC 完整能力

### 实现内容

- PAC 自动生成。
- PAC 原生导入。
- PAC JavaScript 编辑。
- PAC 执行测试。
- 本地 HTTP Server。
- Windows PAC URL 配置。
- PAC 与 Xray 路由联合测试。
- PAC 缓存诊断。

### 验收要求

- PAC DIRECT / PROXY 正确。
- 不同域名经过正确的 Xray 出站。
- PAC 服务不可用时不错误地启用系统 PAC。
- 自定义 PAC 原始内容保留。
- PAC 异常可以恢复。
- 不生成未经授权的 DIRECT 回退。
- 企业 PAC → Global → 本程序 PAC → Direct 后恢复最初的企业 PAC，切换失败恢复事务前态。
- HTTP 入站端口变化时生成新 PAC 版本并切换 URL；版本不匹配、无法确认的自定义端点关联均拒绝提交。
- 统一域名 Direct 在 PAC 与 Global 模式下都保持直连；原生规则遮蔽冲突必须可见。
- 旧 PAC URL 在仍被系统设置引用时持续可用，下载成功但哈希不符不能作为健康通过。
- 启动时 PAC 服务失效且不自动连接，仍条件释放系统 PAC；不能只恢复 Owned 记账。

---

## Phase 4：产品化与可靠性

### 实现内容

- Windows 安装包。
- 自动更新。
- Xray Core 更新。
- 配置升级。
- 日志管理。
- 安全加固。
- 自动化测试。
- 用户文档。
- 故障诊断工具。

### 验收要求

- 安装、升级、卸载流程正常。
- 不丢失用户配置。
- Core 更新失败可恢复。
- 系统代理不会因正常卸载残留。
- 长期运行稳定。

---

## Phase 5：TUN 与高级网络能力

### 实现内容

- Xray 原生 TUN。
- 高权限 Helper。
- Windows 路由管理。
- DNS 状态管理。
- IPv6 支持。
- 网络异常恢复。
- 防泄漏扩展。
- Helper 持有提权 Core、受保护版本与配置、网络前态日志和唯一写入者计划。

### 验收要求

- TUN 可以正常启停。
- 路由无回环。
- 网卡切换可以检测。
- 休眠唤醒后可以恢复。
- 异常退出后能够修复网络。
- 未授权时不执行高权限网络操作。
- 拒绝 UAC 或 Helper 准备失败时不停止旧 Core，不改变已提交代次。
- 分别结束 GUI、Supervisor、Core、Helper，核对 Job 清理及两份恢复结果；网络冲突不盲目覆盖。
- 提权链拒绝可被普通用户替换的 exe/DLL、越界资源路径、非白名单请求及未固定的配置。

---

# 20. 验收矩阵

| 场景 | 预期结果 | 阶段 |
|---|---|---|
| 原生 JSON 加载 | Core 直接执行 | P0 |
| JSON 不合法 | 拒绝启动 | P0 |
| Core 启动失败 | 显示错误 | P0 |
| A/B 双出站 | 同时配置和使用 | P1 |
| 指定域名访问 B | 实际出口为 B | P1 |
| B 故障 | 不自动访问 A | P1 |
| 修改配置失败 | 恢复旧配置 | P1 |
| 成功应用后重启 | 正式 JSON、冻结副本及清单 Revision/哈希一致 | P1 |
| 来源证书或 geo 文件在校验后被更新 | Frozen 代次仍使用同一资源集，运行与回滚不读取新来源内容 | P1 |
| 冻结副本缺失或哈希错误 | 报告 RESOURCE_INTEGRITY_FAILED，不用最新来源文件冒充旧资源 | P1 |
| Dynamic 文件变化后自动重试 | 报告待重新校验，通过新事务应用，不在旧代次静默重启 | P1 |
| 原生扩展依赖无法完整枚举 | 保留字段并标注 CoverageIncomplete，不宣称可完全复现 | P1 |
| 绝对路径无法绑定冻结内容 | 展示原生路径修改或显式动态策略，不能隐藏改写并声称已冻结 | P1 |
| 首次启动无旧代次，应用失败 | 恢复 Stopped/Unmanaged，不伪造 PreviousActive | P1 |
| 正式资产部分替换后退出 | 清单仍旧时，按提升凭据恢复旧资产，保留外部修改 | P1 |
| 清单原子替换前/后退出 | 前者恢复旧代次，后者完成新代次记账；替换结果不明确时读回判定 | P1 |
| 补偿失败或恢复资产缺失 | 保留日志、报告 RecoveryRequired，拒绝新运行写操作 | P1 |
| Applying 候选 Core 异常退出 | 由所属事务补偿，不独立触发旧代次重启或租约释放 | P1 |
| 计划停止的旧 Core 返回退出事件 | 确认所属事务步骤，不进入故障重试 | P1 |
| 未知 JSON 字段 | 不丢失 | P1 |
| URL 缺少入站或网络上下文 | 相关规则不能确定时返回 Unknown | P1 |
| 前序原生规则不受支持 | 不跳过它声称命中后续规则或默认出口 | P1 |
| 静态模拟命中 | 仅展示预期出站，实际出口仍为 Unverified | P1 |
| Windows Global | 正确启用 | P2 |
| Global 下 HTTP 入站端口变化 | Core 与 Windows 目标共同更新，失败恢复事务前态 | P2 |
| 缺失 HTTP 入站或端点不兼容 | 拒绝接管，不隐式补写 JSON 或使用固定端口 | P2 |
| 恢复原有 PAC | 原配置完整恢复 | P2 |
| 持有租约期间切换 Profile | leaseId 和根快照不变，仅更新租约修订与代次 | P2 |
| Windows 写入成功、租约记账前退出 | 用 pendingTransition 及清单完成恢复，不能丢失前态 | P2 |
| IPC 超时后重试同一 requestId | 返回同一操作结果，不重复接管或提交 | P2 |
| 清单已提交、回包前退出，且关闭自动连接 | 重建原请求 Succeeded；不启动 Core，条件释放失效租约 | P2 |
| 完成的旧代次仍持有租约，关闭自动连接后启动 | 即使无未完成事务也检查租约，恢复根快照并报告实际 Stopped/Unmanaged | P2 |
| 未提交事务恢复时关闭自动连接 | 恢复旧资产但不短暂启动旧 Core；租约按安全清理释放 | P2 |
| 自动连接失败或冻结资源不可用 | 条件释放失效代理，独立报告配置提交事实与实际运行失败 | P2 |
| 启动时第三方已修改代理 | 保留外部设置，标记 Conflict，不自动接管 | P2 |
| Released 后回放历史 finalize | 不复活旧租约；重新接管使用新 leaseId 和新运行事务 | P2 |
| g1 退避中提交 g2，再触发 g1 定时器 | 丢弃过期任务，g2 的 Core 与代理不变 | P2 |
| 同一代次重启后收到旧实例退出事件 | runtimeInstanceId 不匹配，丢弃事件 | P2 |
| 新 Supervisor 收到旧 epoch 的事件 | 不执行重启或代理写入，重新核对实际状态 | P2 |
| 提交 g2 后响应丢失，重启后以旧 expectedGeneration 重试 | 去重先于版本检查，返回原 g2 成功结果 | P2 |
| GUI PID 变化后查询同一 requestId | 在稳定 callerKey 范围内返回原记录 | P2 |
| 同一 requestId 对应不同 payload 或候选摘要 | REQUEST_ID_CONFLICT，保留原记录，不产生副作用 | P2 |
| Accepted 登记后、执行意图前退出 | 记录 INTERRUPTED_BEFORE_APPLY，不在重启后自动执行 | P2 |
| 原请求提交成功但终态记账失败 | 从当前清单重建结果，凭据落盘前禁止下一代次覆盖清单 | P2 |
| 终态详情按保留策略压缩 | 返回原终态摘要及 ResultDetailsExpired，不重新执行 | P2 |
| 操作记录损坏或索引重建证据矛盾 | OPERATION_RECOVERY_REQUIRED，不当成新请求执行 | P2 |
| GUI 崩溃 | Supervisor 继续运行 | P2 |
| Core 崩溃 | Supervisor 检测与处理 | P2 |
| 外部代理修改 | 不盲目覆盖 | P2 |
| PAC 脚本生成 | 结果正确 | P3 |
| PAC 自定义导入 | 保留原内容 | P3 |
| PAC HTTP 服务 | 正常发布 | P3 |
| PAC 与 Xray 协同 | 实际出站正确 | P3 |
| 企业 PAC → Global → 本程序 PAC → Direct | 最终恢复首次接管前企业 PAC、bypass 及自动检测状态 | P3 |
| 已有 Global 租约内 PAC 切换失败 | 恢复 Global 前态，保留同一根快照；外部修改则 Conflict | P3 |
| PAC 模式下 HTTP 端口变化 | 新 PAC 内容、不可变 URL、Core 端点及清单匹配 | P3 |
| 自定义 PAC 端点关联无法确认 | 拒绝自动联动，保留原脚本供显式修订 | P3 |
| PAC 发布可访问但内容哈希不符 | 不切换系统 PAC，不提交代次 | P3 |
| 不自动连接且 PAC 发布已失效 | 条件恢复原系统代理/PAC，不仅将旧租约记为 Owned | P3 |
| 系统仍引用旧 PAC URL | 保留旧发布及必要监听，不能提前清理 | P3 |
| 域名 Direct 在 PAC/Global 下访问 | PAC 返回 DIRECT，进入 Xray 时明确选择 freedom | P3 |
| Direct 规则被原生规则遮蔽 | 展示冲突，未解决前不声明统一策略生效 | P3 |
| 自定义 PAC 执行超时/不支持函数 | 返回 Error/Unsupported，不自动降级 DIRECT | P3 |
| 安装卸载 | 正常清理 | P4 |
| Core 更新失败 | 可以回滚 | P4 |
| TUN 无权限 | 显示权限错误 | P5 |
| 拒绝 UAC | 保持旧代次和旧 Core，不产生网络修改 | P5 |
| TUN 正常运行 | 提权 Core 的进程及 Job 由 Helper 持有，Supervisor 仅持运行标识 | P5 |
| TUN Supervisor 中断 | Helper 停止 Core 并验证网络恢复，用户代理另行条件恢复 | P5 |
| Helper 强制结束 | Job 终止 Core，未恢复网络由下次提权日志恢复，不提前报告成功 | P5 |
| 替换用户可写 exe/DLL 或请求越界路径 | Helper 拒绝执行，配置内容不被隐式改写 | P5 |
| Core 原生自动路由与 Helper 同时计划写同一项 | 准备阶段拒绝重复写入者 | P5 |
| TUN 异常退出 | 网络可恢复 | P5 |
| 网卡切换 | 不出现路由回环 | P5 |

---

# 21. 风险与限制

## 21.1 Xray Core 版本变化

风险：配置结构变化。

策略：

- 原生 JSON 透传。
- 使用目标 Core 实际校验。
- 保留旧版本。
- 升级前检查兼容性。

## 21.2 Windows 代理竞争

风险：其他程序同时修改系统代理。

策略：

- 使用 Proxy Lease。
- 比较当前预期状态。
- 冲突时不自动覆盖。
- 提供恢复工具。

## 21.3 PAC 缓存

风险：PAC 更新后部分应用未立即生效。

策略：

- PAC Revision。
- HTTP 缓存控制。
- 真实客户端测试。
- 提示缓存状态。

## 21.4 域名路由不可识别

风险：应用只提交 IP，或目标域名不可嗅探。

策略：

- 优先保留域名信息。
- 使用 Xray Sniffing。
- 提供静态与真实路由测试。
- 明确严格路由的能力边界。

## 21.5 系统网络恢复

风险：TUN 或防火墙修改导致网络异常。

策略：

- 高权限操作独立。
- 保存原始状态。
- 执行网络恢复验证。
- 提供独立修复工具。

## 21.6 跨资产提交与中断恢复

风险：正式配置、PAC、Windows 代理和 Core 进程不存在跨资源原子事务。

策略：

- 按 §5.4 使用单一清单提交点、不可变运行计划与写前日志。
- 在每个持久化边界、进程切换、Windows 写入及清单替换前后注入故障，核对恢复方向。
- 外部文件或代理修改触发冲突，恢复失败保留证据并阻止继续切换。
- 同时保留已提交状态与实际健康状态，不能把“提交成功”解释为客户端缓存更新或远端网络正常。

## 21.7 恢复任务、资源与请求身份

风险：旧重试任务、可变文件依赖或丢失的请求索引可能让已经完成的切换被过期动作破坏。

策略：

- 启动安全清理独立于自动连接，恢复结果与历史请求提交事实分别记录。
- 所有事件和重试在协调器内核对当前代次、实例及 epoch，旧任务不跨启动复用。
- 依赖清单区分冻结输入和动态边界，资源变化不能伪装成同一代次的完整恢复。
- 在副作用前保存请求身份及事务关联，先持久去重再检查执行版本，保留原终态凭据。

---

# 22. 架构冻结建议

建议冻结以下接口合同。冻结的是职责与行为约束；具体 Rust 类型、异步取消和 Windows API 实现仍须通过阶段验收，不能将接口示意直接视为实现完成。

### 22.1 NativeConfigStore

保证：

- 原生 JSON 权威。
- 未知字段保留。
- Revision 检查。
- Core 校验。
- 冻结候选、条件提升及恢复凭据；不单独提交运行代次。
- 历史回滚通过新的运行事务执行。
- 资源清单参与候选摘要及验证，Frozen/Dynamic/CoverageIncomplete 的恢复范围明确。

### 22.2 RuntimeController

保证：

- Supervisor 唯一控制。
- Core 生命周期明确。
- Job Object 管理。
- 运行健康分级。
- UserProcess 与 ElevatedTun 后端分离，后者由 Helper 持有高权限进程及 Job。
- RuntimeHandle 绑定具体实例和监督者 epoch；仅由协调器处理事件及执行重启。

### 22.3 WindowsProxyController

保证：

- Direct / Global / PAC。
- 系统快照。
- 租约管理。
- 外部修改检测。
- 安全恢复。
- 接管期间根快照不可变，区分事务前态补偿与根快照释放。
- pendingTransition、完整状态比较和提交后的租约修订确认。
- 启动失效租约清理不受自动连接关闭影响，Released 凭据禁止历史 finalize 复活租约。

### 22.4 PacService

保证：

- PAC 自动生成。
- PAC 原生导入。
- PAC 测试。
- PAC HTTP 发布。
- Revision。
- 故障处理。
- 不可变版本 URL 与内容哈希探测；保留仍被系统或恢复流程引用的旧发布。
- 不单独修改 Windows 设置或推进 Active。

### 22.5 Supervisor IPC

保证：

- 消息版本化。
- 当前用户鉴权。
- 操作串行化。
- 配置并发保护。
- 结构化错误。
- expectedGeneration 检查、requestId 重试幂等及操作结果查询。
- 稳定 callerKey、完整请求指纹及副作用前持久登记；去重先于执行版本检查。
- 跨重启恢复原请求结果，压缩终态记录不允许旧请求重新执行。

### 22.6 Diagnostics

保证：

- 配置校验与运行校验分离。
- 静态路由与真实出口验证分离。
- PAC 模拟与真实客户端执行分离。
- 输入上下文与能力清单明确；前序未知规则阻止确定性推导。
- MatchKnown/Unknown 与实际出口证据独立展示。
- resourceSetId 与依赖覆盖范围进入诊断上下文，历史成功与实际健康分别展示。

### 22.7 Windows Network Helper

保证：

- 高权限操作隔离。
- 操作白名单。
- 用户明确授权。
- 网络状态恢复。
- 不参与普通用户态基础运行链。
- 持有提权 TUN Core 的进程及 Job，监督失联清理。
- 受保护的 Core/DLL/配置及资源路径约束。
- 每项网络变更的唯一写入者与独立恢复日志。

### 22.8 GenerationCoordinator

保证：

- Supervisor 串行协调 Core、JSON、PAC 与系统代理，统一运行代次。
- Revision 与 generationId 分离，正式清单原子替换是唯一提交点。
- 持久化前态、逐步补偿、提交结果读回判定与中断恢复。
- 提交前失败恢复前代次；提交后回退通过新事务执行。
- RecoveryRequired/Conflict 可观察且不会被新操作覆盖。
- 启动恢复核对全部遗留租约，自动连接关闭仍执行条件释放。
- 候选故障归属事务；重试每次核对 generationId、runtimeInstanceId 及 epoch。
- 下一代次提交前保存上一事务的提交及操作结果凭据。

### 22.9 文件资源与 OperationStore

保证：

- 不可变资源清单、逐文件哈希及被引用资源保留，校验/启动/回滚使用同一批输入。
- 动态或未覆盖依赖显式限制自动恢复，TUN 不接受未冻结的外部输入。
- 操作身份、请求摘要、事务关联、提交证据与终态摘要持久保存。
- 索引可重建，无法确定是否已执行时阻止重复执行并保留证据。

---

# 23. 最终架构决策

| 项目 | 决策 |
|---|---|
| 项目类型 | Windows Xray 原生配置与运行管理器 |
| 开发语言 | Rust |
| GUI | egui |
| 代理执行 | Xray Core |
| 配置权威来源 | 原生 JSON |
| 配置管理 | 原生编辑 + GUI 编辑 |
| 多出站 | 原生支持 |
| 域名路由 | Xray Routing |
| Windows 代理 | Direct / Global / PAC |
| PAC 管理 | 独立 PAC Manager |
| Core 生命周期 | Supervisor 协调；普通 Core 由 Supervisor 持有，TUN Core 由 Helper 持有 |
| Windows 进程控制 | Process API + Job Object |
| GUI 与 Core | 生命周期解耦 |
| IPC | Windows Named Pipe |
| IPC 重试 | 持久 OperationRecord；先去重后版本检查，跨重启恢复历史结果 |
| 系统代理控制 | WinINet |
| 系统代理恢复 | Proxy Lease：不可变根快照 + 转换前态 + 条件恢复 |
| 启动安全清理 | 独立于 connect_on_start，失效服务对应的租约条件释放 |
| 登录自启 | Task Scheduler |
| 高权限操作 | 独立 Helper，受控启动提权 TUN Core 并负责网络恢复 |
| TUN | Xray 原生 TUN，后续阶段 |
| DNS | 首版不强制修改全局 DNS |
| 配置校验 | Xray Core 实际验证 |
| 运行提交 | GenerationPlan + 写前日志 + Active Manifest 唯一提交点 |
| 文件依赖 | ResourceManifest + 冻结资源集；动态引用明确限制恢复保证 |
| 故障重试 | 协调器核对代次、运行实例和 epoch，丢弃过期任务 |
| 故障恢复 | 条件补偿 + 中断恢复表 + 配置/代理/网络独立结果 |
| 路由诊断 | 上下文明确的静态模拟 + Unknown 边界 + 实际出口证据 |
| 统一域名 Direct | PAC DIRECT 与 Xray freedom 规则同时生成并检查遮蔽 |
| Modorch-RS | 首版不强制引入 |

---

# 24. 官方参考文档

**Xray Core**

- [Xray Core 官方仓库](https://github.com/XTLS/Xray-core)
- [Xray Core 配置规范](https://xtls.github.io/config/)
- [Xray Core 路由配置](https://xtls.github.io/config/routing.html)
- [Xray Core 命令行](https://xtls.github.io/document/command)
- [Xray VLESS 出站](https://xtls.github.io/config/outbounds/vless.html)
- [Xray TUN 入站](https://xtls.github.io/config/inbounds/tun.html)
- [Xray Windows TUN 实现](https://github.com/XTLS/Xray-core/blob/main/proxy/tun/tun_windows.go)

**Windows**

- [WinINet API](https://learn.microsoft.com/en-us/windows/win32/wininet/)
- [Windows Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects)
- [Windows Task Scheduler](https://learn.microsoft.com/en-us/windows/win32/taskschd/task-scheduler-start-page)
- [Windows WinHTTP AutoProxy](https://learn.microsoft.com/en-us/windows/win32/winhttp/winhttp-autoproxy-api)
- [Windows Data Protection API](https://learn.microsoft.com/en-us/windows/win32/seccng/data-protection)

---

# 25. 设计结论

Xray Windows Manager 的核心目标是建立一个对 Xray Core 原生配置具有完整控制能力的 Windows 桌面管理程序。

其整体实现遵循以下方向：

1. Xray Core 作为唯一代理执行引擎。
2. 原生 JSON 作为 Xray 配置的唯一权威来源。
3. Windows GUI 与运行控制分离。
4. Supervisor 统一管理 Core、PAC 和系统代理。
5. Windows 系统代理通过受支持 API 集成。
6. PAC 与 Xray Routing 分工明确。
7. 配置、Core、PAC 和系统代理通过运行代次联合验证与提交，以持久化清单明确提交和恢复方向。
8. 系统代理在同一次接管期间保留首次快照，转换补偿与退出恢复采用不同的明确目标。
9. TUN Core 由提权 Helper 受控持有，高权限进程与网络恢复责任不转移给 GUI。
10. 统一域名动作保持 PAC/Xray 语义一致；路由诊断区分静态预期、未知结果及真实证据。
11. 所有功能分阶段实现，以中断恢复、模式切换及权限故障场景验证并验收。
12. 关闭自动连接仍完成失效租约清理；过期故障任务不能修改新运行代次。
13. 配置恢复覆盖明确冻结的文件输入，动态依赖变化通过重新校验和新事务处理。
14. 请求身份与执行证据持久保存，重试查询原结果，不因重启而重复执行。

**最终架构：Rust GUI + Runtime Supervisor + Native Xray Core + Windows Proxy/PAC Integration + Optional Privileged Network Helper。**

本设计为实施基线，不表示各组件已经开发完成。所有 Windows API 行为、Xray 配置示例和网络恢复机制均应在目标 Windows 构建版本与实际 Xray Core 版本上完成测试后再标记为已支持。
