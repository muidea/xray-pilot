# Phase 0–2：C13–C15 收口后遗留核对

日期：2026-11-10。以当前工作区源码和 [C13–C15 收口记录](phase0-2-c13-15-closeout.md) 为基线。本轮核对不修改产品代码，另按用户要求刷新面向使用者的 README。

**后续状态：C16 已修复并纳入正式回归，见 [C16 收口记录](phase0-2-c16-closeout.md)。本文保留修复前的代码行为、探针和文档刷新记录。**

## 结论（修复前）

新增 **C16 / P2 / 已复现**：已有 Profile 时，正常 GUI 启动会丢弃已保存的 Settings 响应。C01–C15 原场景保持关闭；C14 的草稿/候选绑定修复没有覆盖设置读取的独立生命周期。V03 保持受控 A/B 范围关闭，V01、V02、V04–V09 和 F01–F05 的未完成部分继续保留，Phase 0–2 尚未全部验收。

## C16：首次 Profile 加载淘汰正常设置响应

代码位置：[启动请求顺序](../crates/xray-manager-app/src/app.rs#L140)、[选择代次变化](../crates/xray-manager-app/src/app.rs#L280)、[统一上下文过滤](../crates/xray-manager-app/src/app.rs#L312)、[首个 Profile 自动读取](../crates/xray-manager-app/src/app.rs#L359)。

正常启动即可触发，不需要用户操作或异常 IPC：

1. 已有至少一个 Profile，Settings 已保存 Core 路径及启用的自启选项。
2. GUI 按顺序发送 GetStatus、ListProfileSummaries、GetSettings。
3. 收到非空 Profile 列表后，GUI 自动发送 GetProfile，立即改变 selected_profile 和 view_epoch。
4. 原 GetSettings 响应绑定的是选择前的完整 EditorContext，被统一的上下文过滤丢弃。
5. GUI 最终显示空 Core 路径、connect_on_start=false、start_on_logon=false；后续 Profile 读取没有补发 GetSettings。

直接影响是每次重开都需重新选择 Core，且 GUI 显示的设置与后台实际设置不一致。单纯打开 GUI 不会改写后台设置；但点击 Save settings 会发送这些默认值，可能覆盖原 Core 路径、关闭自动连接，并通过既有设置事务取消本实例的登录任务。最后一项的实际 Windows 任务删除本轮未执行；探针已确认发送的 SaveSettings 载荷，后端对应处理路径为静态核对。

### 复现与对照

通过临时测试包含实际 `app.rs`，用 `App::with_channels` 和真实 request ID 驱动状态处理；不复制处理逻辑，不创建窗口或任务。

- 已有 Profile：按照真实启动顺序注入列表、Settings 和 Profile 响应，最终 Core 路径为空、两个选项均为 false；继续 SaveSettings 得到相同默认值载荷。
- 空 Profile 列表：使用相同的非默认 Settings，路径与两个启用选项均正确加载，作为对照。
- Linux 和 Windows GNU 原生执行均复现。各平台同时运行原有 5 项 GUI 状态回归，共 7 个测试通过。新增探针断言的是当前错误行为，**通过表示复现成功，不表示 C16 已修复**。

本机证据：

- `/tmp/xray-pilot-post-c15-gui-review.log`
- `/tmp/xray-pilot-post-c15-gui-windows-review.log`
- `/tmp/xray-pilot-post-c15-gui-all.log`
- `/tmp/xray-pilot-post-c15-gui-windows-all.log`
- `/tmp/xray-pilot-post-c15-review-probes/gui.rs` 与同目录复现说明

临时测试已从 crate 的测试发现目录移除，产品代码未修改。本轮无需重复上一轮的 Core、IPC、租约及整套平台验收，既有证据见收口记录。

### 关闭条件与临时处理

设置读取应绑定自己的请求代次及设置编辑版本。Profile 选择、加载或 JSON 编辑不应使未被设置编辑取代的 Settings 响应失效；用户已经修改的设置也不能被迟到响应覆盖。补充已有 Profile 的首次启动、空列表、选择切换、设置编辑与迟到读取的回归，并确认保存载荷保留正确值。

修复前，重开后在 Settings 重新选择 Core，并按原设置核对两项开关，再决定保存。不要把当前显示的默认值当作后台真实值；已有运行配置可继续通过后台和 CLI 管理。

## 本轮未列为缺陷的候选

直接在状态对象中模拟“Apply 期间继续编辑”“Delete 期间选择另一个 Profile”能够触发回调覆盖，但实际 `App::update` 在 busy 时禁用了整个中央编辑区（app.rs 第 784 行）。这些探针绕过了真实交互限制，因此不作为可触发缺陷，不新增问题编号。

安全停止日志落盘失败仍属于 V04/V05 待补的故障矩阵；本轮只有源码路径检查，没有故障注入，也未据此宣布新缺陷。长业务事务仍串行执行、Direct 的就绪范围以及缺少 GC 均为已声明限制，不重复计为新问题。

## 文档与交付

README 已按安装、Core 准备、首次配置、连接验证、日常操作、登录自启、后台部署、备份更新与常见问题重写；支持直接使用 `C:\v2rayN\bin\Xray\xray.exe`。技术验证及高级请求示例移至 [开发说明](development.md)，最新遗留项统一链接到本报告及 [清单](phase0-2-backlog.md)。

程序包仅刷新 README 和文档，三个 release 可执行文件保持上一轮已验证版本，因此仍包含 C16。文件校验和以 `dist/SHA256SUMS` 为准。本轮未改变真实 WinINet、登录任务或用户代理进程，亦未把 README 的部署说明视为新的验收证据。
