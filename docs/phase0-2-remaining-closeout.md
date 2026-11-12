# Phase 0–2 后续能力收口

日期：2026-11-11。范围为 F01–F03 / F05 的可实现部分；当前源码尚未提交、打包或发布，已发布的 rc.4 不包含这些变更。C01–C20 的既有关闭结论保留，V01 / V02 / V04–V09 的系统验收缺口不因本轮常规测试而关闭。

本轮遗留收口复测（2026-11-11）使用当前源码交叉构建的 Windows GNU 可执行文件和用户指定的官方 Core `C:\v2rayN\bin\Xray\xray.exe`。隔离 smoke 通过 Direct 应用、Core Job 强杀清理、重启去重、托盘打开/隐藏/退出、后台托盘 Quit 以及系统代理保持不变；便携目录专项通过 GUI、独立 Supervisor、CLI 和依赖脚本均使用安装目录旁 `data`，与工作目录无关。该证据仍不覆盖真实 WinINet 往返、登录任务权限拒绝、断电/磁盘满/安全软件、跨 SID/远端访问、远端 CI 或完整人工桌面交互。

## 已补齐

- F05：托盘配置子菜单显示配置名称及运行 / 编辑状态，选择后打开配置页；有草稿时先确认，仍需检查并确认应用。表单可选择、添加、删除多个服务器、VLESS / VMess 用户及 SOCKS / HTTP 账号，保留相邻条目与扩展字段。
- F05：原生数值的未完成输入随每份草稿保存，绑定原值和 JSON 路径；切换、重开、后台应用、写盘失败重试均保留。补全前不能检查应用、复制 / 导出配置、覆盖原始 JSON 或删除 / 重排结构，提供字段定位。明确放弃草稿才删除输入；退出失败时的草稿导出包含未完成输入。
- F02：DNS hosts、servers.domains 的 geosite / ext 与 servers.expectIPs 的 geoip / ext 依赖冻结；常见 DNS 字段与 RAW 传输进入覆盖判断。源文件后续变更不影响冻结版本，丢失、越界和未知扩展仍拒绝或限制恢复。
- F03：Direct / Global 启动均在发布前等待常用回环 TCP / RAW HTTP、SOCKS5 入站的本地握手，支持 Basic / SOCKS 账号认证。wildcard bind 使用回环地址探测；HTTP 使用回环目标的 CONNECT，SOCKS 在认证完成后关闭，不请求目标连接。探测有总期限、分组并发上限、提前退出监测和取消清理。Global 另保留原无认证 HTTP 入口验证。
- F01：提供 `xrayctl maintenance` 离线预览、显式回收及按需终态压缩，详见下节。
- F01：维护读取设置事务终态证据时现在校验完整的 `before`、`target`、`task_before`、`task_target` 和 `outcome` 结构；缺字段、错误类型或伪造终态均 fail-closed，不会继续回收孤儿产物。
- F05：SOCKS / HTTP 上游已补充账号数组中 `user` / `pass` 的非阻断字段提示；账号字段类型错误也会定位到 `/users`；不会回显账号或密码。

## 离线维护合同

`maintenance [--retain-days DAYS] [--compress-operations] [--apply]` 直接读取已存在数据目录，不经 IPC，不启动 Core，也不调用系统代理或登录任务后端。运行前应通过“退出程序”停止后台并备份整个数据目录。

默认仅预览，保留期 7 天，最少 1 天。持有 Supervisor 同一独占锁直到结束；任何未终态 / 待恢复操作、未结算运行 / 发布 / 设置事务、恢复文件、未释放或不一致的代理租约、损坏证据、符号链接 / Windows reparse point、特殊文件或未知资产名称均保守阻断。维护不修复证据，也不删除异常记录。

引用根包括全部完整操作记录、配置 / GUI 设置 / 草稿、运行提交 / 事务 / 恢复元数据和 snapshots。引用分析解码 JSON 及其中嵌入的 JSON 字符串，保守匹配产物 ID，随后沿可达候选 / 资源元数据传播引用；假阳性只会多保留。未引用 manifest 不因包含自身 ID 而永久自保留。Core 名称限定 SHA-256 形状，候选 / 资源名称限定 UUID 形状，异常名称要求人工核对。

仅 `candidates`、`resources`、`cores` 中无引用且整个子树最新修改时间已超过保留期的目录可以回收。预览列出相对路径、字节数和 `referenced` / `recent` / `orphan`；应用前重新检查目录树是否变化。部分删除中断只影响已经确认无引用的准备产物，引用证据、身份与历史始终保留。

`--compress-operations` 单独启用旧终态操作压缩，仅处理超过同一保留期、压缩后实际缩小的记录。完整请求、响应、摘要、事务和记录哈希保留在带长度 / SHA-256 的 gzip JSON envelope 中，原路径以现有原子写入机制替换；读取兼容普通记录和 envelope，限制解压大小并继续校验身份 / 记录哈希。损坏、截断、长度不符或未知版本均阻断重放。GetOperation / 完整详情与历史继续读取解压后的同一 Operation，不重复执行副作用。

**压缩是显式的数据格式升级，旧程序不能读取 envelope。降级必须恢复压缩前的完整备份。** 默认维护不压缩操作，不删除 request-identities、事务、提交、恢复记录或历史。依赖下载缓存、已安装版本、历史过期和自动容量上限尚未提供，因此 F01 保持“部分实现”。

## 验证

| 检查 | 结果 |
| --- | --- |
| Linux 全工作区常规回归 | 155 通过，6 项 opt-in 默认忽略；其中 GUI 73 项 |
| 最终 GUI 回归（补充数值定位 / 布局专项后） | Linux 73 通过；Windows 原生 75 通过；各有 1 项官方模板专项默认忽略并另行验证 |
| 最终维护专项 | Linux 6 项与离线 CLI 1 项通过；覆盖损坏 effect/settings 终态证据 fail-closed；Windows 维护专项含 junction 拒绝与压缩后原请求重放，纳入下行 Supervisor 回归 |
| Windows 原生常规回归 | Supervisor 23、配置 14、Runtime 6、Windows 集成 8 项通过；GUI 见上行 |
| Windows 官方 Core 专项 | 8 项通过：Direct / Global 的 HTTP / SOCKS 认证就绪、原校验 / 应用 / 去重 / 释放流程、租约丢失修复、候选 / 历史发布恢复、混合重连预算、文件占用补偿、A/B 真实流量禁止回退、七模板及附加传输配置校验 |
| 严格静态检查 | Linux 与 Windows GNU 全工作区 / 全目标 Clippy（`-D warnings`）、fmt、diff 空白检查通过；变更文档本地链接检查通过 |

本轮 Windows 原生收口日志由 `scripts/windows-smoke.ps1`（含 `-Gui`）和 `scripts/windows-portable-check.ps1` 生成；测试使用隔离临时数据目录，结束后未保留后台进程或系统代理修改。

最后一项 GUI 布局回归模拟最小窗口滚动到编辑区，点击“定位未完成数值”，确认跳回正确标签与字段、输入不丢失，深浅主题及三种尺寸下无文字重叠。Windows 时间测试夹具首次因只读句柄不允许设置修改时间而失败，改为写权限句柄后专项及完整 Supervisor 原生回归通过。

Windows 测试采用 GNU 目标构建后原生执行，使用隔离临时目录及已校验的官方 Core 缓存；系统代理使用模拟后端，真实 WinINet 和登录任务未修改。常规 Windows 集成中的原生登录任务测试仍默认忽略；未运行的系统专项不据此标记为通过。该证据不替代 MSVC CI、最终发行包及 V08 的完整桌面交互验收。

本机原始日志位于 `/tmp/xray-remaining-workspace-final.log`、`/tmp/xray-remaining-gui-final-verified.log`、`/tmp/xray-maintenance-final-tests.log`、`/tmp/xray-remaining-windows-native.log`（配置 / Runtime / Windows 集成通过证据及首次夹具失败）、`/tmp/xray-remaining-windows-native-final.log`、`/tmp/xray-remaining-windows-gui-final.log`、`/tmp/xray-remaining-official-extra.log` 及 `*-clippy-*-verified.log`。这些是本机验证日志，不随发布包交付。

## 继续保留的边界

- F02：复杂传输及所有原生扩展并未覆盖，绝对文件依赖路径仍要求显式修改；表单支持不等于依赖能力全覆盖。
- F03：无入站配置仍只验证 Core 校验及短时进程存活；非回环绑定、TLS、特殊传输、Proxy Protocol、非数值端口和其他协议仅输出检测未覆盖的日志。Running 和本地握手不证明远端出口可用。
- F05：更多协议表单、复杂组合规则可视化与冲突分析、订阅 / 分享链接及延迟探测继续保留；V08 仍需完整原生交互验收。
- 本轮新增校验提示仅覆盖已识别的 SOCKS / HTTP 账号字段；Core 版本相关的其他认证语义仍由所选官方 Core 校验，不能据此宣称所有协议字段均已覆盖。
- V01 / V02 / V04–V09：真实 WinINet、实际登录 / 权限拒绝、断电 / 磁盘满 / 安全软件、跨 SID / 远端拒绝、完整重连故障矩阵、GUI 全流程及当前提交的远端 CI / 最终发布包验收仍待相应环境与证据。
