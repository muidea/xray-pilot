# Phase 0–2 当前遗留复核（2026-11-10）

> 本记录保留修复前复现与当时的验证状态；C18–C20 随后已修复，当前结论见 [收口记录](phase0-2-c18-20-closeout.md) 和 [遗留清单](phase0-2-backlog.md)。

核对基线：提交 `73966ff` 及其后的本地修改，包含运行错误提示刷新和验证包命名调整。五页界面、配置导入和布局调整已在该提交中；提示刷新及命名调整尚未提交，既有验证包没有包含提示刷新修复。本轮没有修改产品代码、提交或生成新包。

**结论：C18–C20 在当前源码仍能复现，不能关闭；F01–F05 的未完成部分和 8 组验收缺口继续保留。** C01–C17 原关闭场景不重新打开。以下定向验证不代表重新验收全部已关闭场景。

## 尚未关闭的代码问题

| ID | 优先级 | 当前实际结果 | 收口要求 |
| --- | --- | --- | --- |
| C18 | P1 | 用普通文件占据 `config/editor-drafts` 后，编辑并从托盘退出；出现写入错误，但 `exit=true`，草稿未落盘。`stash_draft` 失败后仍清除保存计时器，窗口内退出和完整退出也不检查保存结果 | 保存返回可检查结果；失败时保留待保存状态、界面和重试 / 导出入口；仅在明确放弃修改后继续退出，覆盖全部正常退出入口 |
| C19 | P2 | 首次设置响应因用户编辑被丢弃；下一次状态轮询自动补读，补读响应将用户选择的 Core 路径改回保存值 | 分离设置基准和用户草稿，自动读取不覆盖修改；补齐“首次迟到响应 → 状态轮询 → 自动补读”回归 |
| C20 | P2 | A 的 S1 应用期间编辑成 S2 并切到 B；应用成功后，内存草稿基准为 S1，磁盘仍为旧基准。重开后 S2 正文保留，但误报冲突 | 基准更新同步原子持久化；覆盖正常响应、原请求查询确认及写盘失败；重开保留 S2 且没有虚假冲突 |

代码位置：C18 为 [stash_draft](../crates/xray-manager-app/src/ui.rs)、[tray_action / advance_shutdown](../crates/xray-manager-app/src/app.rs)、[execute_action](../crates/xray-manager-app/src/ui/dialogs.rs)；C19 为 `app.rs` 的设置响应上下文检查、`GetStatus` 补读和 `GetSettings` 赋值；C20 为 `ui.rs::applied_draft` 非当前配置分支。具体生命周期与关闭条件沿用 [原复核](phase0-2-post-ui-review.md)。

## 已修复及交付状态

- 运行成功后旧 `PROXY_ENDPOINT_INVALID` 提示残留：代码修复已有回归证据。新运行操作成功、结果已保存且无恢复阻断时清除后台旧错误；GUI 同步清理请求失败，本地写入错误和历史失败保留。详见 [修复记录](ui-implementation.md#运行错误修复后的提示刷新2026-11-10)。这不关闭 C18–C20。
- 验证包命名：脚本已采用 `xray-pilot-<version>-windows-x64.zip`，同名包拒绝覆盖。版本目前仍为 `0.1.1-rc.3`；后续新内容交付应递增版本（建议 `0.1.1-rc.4`），同步 Cargo.toml / Cargo.lock、构建和包内元数据。本轮不升级版本、不重新交付。
- 新界面已提交到 `73966ff`，不能继续描述为“新界面尚未提交”。已发布 rc.3 的旧提交 CI 不能替代该提交及当前修改的验收；当前尚无对应完整源码快照的远端 CI 证据。

## 能力与验收缺口

| 类别 | 仍需完成 |
| --- | --- |
| F01 | Core / 资源 / 候选的安全回收、容量与保留策略、操作终态压缩和详情过期；不能直接删除请求身份或恢复证据 |
| F02 | DNS、复杂传输和更多原生字段的依赖覆盖，绝对资源路径仍需调整；覆盖不完整时保持自动恢复限制 |
| F03 | Direct 当前只在启动约 250 ms 后检查是否退出，未证明全部入站就绪；Global 的本地 HTTP 握手不证明远端出口可用 |
| F04 | Core `run -test` 失败仍只返回通用错误及退出码，缺少可用的临时原始诊断通道 |
| F05 | 结构化对象字段增删、数值输入缓冲、出站显示名称、托盘 Profile 快捷列表；数值文本每帧从 JSON 重建，负号 / 小数点等未完成输入可能丢失 |
| V01 / V02 | 真实 WinINet 往返和外部修改；实际登录启动与权限拒绝 |
| V04 / V05 | 全部副作用边界的系统故障矩阵、磁盘满、安全软件干扰及断电恢复 |
| V06 / V07 | 跨 SID / 远端 / 恶意服务端及压力；更多完整性、取消、重连和真实 Global 组合 |
| V08 / V09 | 全部原生桌面交互；当前源码对应 CI、最终构建和交付包复测；既有并行测试不稳定的原因仍未定位 |

V03 继续仅在受控 A/B 上游范围关闭。完整逐项证据见 [遗留清单](phase0-2-backlog.md)。PAC、安装器 / 自动更新、TUN / Helper 属于后续阶段，不计入此次 Phase 0–2 缺陷数。

## 本轮验证与证据边界

三个临时探针均确认当前错误行为，日志为 `/tmp/xray-current-remaining-probes.log`：

- `remaining_review_probe_gui_exit_proceeds_after_draft_write_failure`
- `remaining_review_probe_settings_retry_overwrites_initial_user_edit`
- `remaining_review_probe_background_apply_leaves_old_persisted_draft_base`

这些探针通过表示缺陷复现成功，不表示修复通过。执行后原测试文件已按字节还原，探针没有加入正式套件。C18 的托盘路径、C19 的完整补读链和 C20 的正常应用响应及重开路径已动态验证；其余退出入口、C20 查询确认共用路径为静态核对。

移除临时探针后，本轮 `cargo test -p xray-manager-app --locked -- --test-threads=4` 正式 GUI 回归 **49 项通过**，日志为 `/tmp/xray-current-gui-review-tests.log`。

近期已留存的同一代码阶段验证：Linux 工作区 116 项通过、4 项专项默认忽略；Windows 原生 GUI 51 项、Supervisor 18 项及官方 Core 专项 1 项通过；两平台全目标严格 Clippy 通过。本轮复用这些记录，没有重新运行整套 Windows 或系统专项。官方 Core 验证使用隔离目录和模拟代理，不代表真实 WinINet 已验收。

此前默认并行回归 `failed_apply_restores_actual_running_or_stopped_state` 的偶发失败没有根因结论；后续整套通过不能据此关闭该不稳定项。

建议顺序：先收口 C18–C20 并补正式回归，再完善诊断 / 编辑体验及系统验收；完成后递增版本，验证对应源码和最终包。
