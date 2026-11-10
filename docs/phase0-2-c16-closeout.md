# Phase 0–2：C16 收口与 v0.1.1-rc.1 验证发布

日期：2026-11-10。对应 [修复前核对](phase0-2-post-c15-review.md) 与 [遗留清单](phase0-2-backlog.md)。发布前先关闭本轮确认的 C16，随后发布 Windows x64 验证版；系统级待验收项保持各自状态。

## 修复结果

已有 Profile 时，GUI 重开能够正确加载保存的 Core 路径、登录自启和自动连接选项；点击保存不会因为 Profile 的自动选择而把原设置替换为默认值。

[App](../crates/xray-manager-app/src/app.rs) 为 Settings 读取保存独立请求代次、设置编辑版本和设置草稿值。Profile 选择、加载、JSON 编辑不改变该上下文；新的设置读取取代旧读取，已发送的保存使先前读取失效，迟到响应不能覆盖用户已经改过或改后撤回的设置。只有成功入队才更新请求/保存栅栏。

同时细分 Profile 读取的匹配条件：正文与历史读取核对选择和草稿；Core 路径从 Settings 加载不再误伤正常 Profile 加载。校验和历史预览继续核对完整 Core、资源、模式及 HTTP Tag 上下文，保留 C14 的候选与应用保护。

## 正式回归

[app_tests.rs](../crates/xray-manager-app/src/app_tests.rs) 新增 5 组正式回归，断言修复后的正确结果：

- 已有 Profile 的真实启动请求顺序，设置先于/晚于正文返回均正确；检查最终 Profile 和 SaveSettings 载荷。
- 空 Profile 列表仍正确加载并保存已有设置。
- 切换 Profile、加载正文和编辑 JSON 不淘汰设置读取。
- Core 路径及两个选项的用户编辑受保护，改后撤回也不会被旧读取覆盖。
- 后发设置读取优先，保存使旧读取失效，且读取响应不误清除写操作 busy 状态。

原有 5 项 C14/队列回归继续保留，GUI 共 10 项。修复前临时探针及日志保留为历史证据，已不在正式测试发现目录中。

## 构建与验证

本轮版本为 0.1.1-rc.1。Cargo.lock 仅更新 6 个工作区包的版本，外部依赖记录按结构化内容核对未改变。

Linux **69 项常规回归**通过；Windows GNU 构建后原生运行 **44 项常规单元回归和 2 项独立 IPC 回归**通过，包含两平台各 10 项 GUI 状态测试。全工作区格式检查、Linux/Windows GNU 全目标严格 Clippy 通过。

三个 Windows x64 release 程序重新构建成功。使用用户提供的 Xray v26.3.27，通过 GUI 启动/关闭隐藏/独立退出/重开、真实 Core/IPC、Job 强杀清理、历史成功重放及系统代理不变检查；IPC 负面脚本和停滞客户端监督验证通过。三种停滞客户端并存时，真实 Core 旧实例退出检测单次观测为 0.243 s，不作为性能保证。远端 Ubuntu/Windows MSVC CI 将由发布提交触发，其结果以仓库 Actions 为准；本机 GNU 验证不替代远端 MSVC。

可重现命令：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked -- --test-threads=4
cargo clippy --workspace --all-targets --target x86_64-pc-windows-gnu --locked -- -D warnings
cargo build --release --workspace --target x86_64-pc-windows-gnu --locked
```

Windows 原生运行时设置 XRAY_TEST_SUPERVISOR 指向本轮编译的 Supervisor；GUI 单元按窗口子系统编译，使用 Start-Process、缓存 Handle 并显式等待 ExitCode。发布前使用本轮 release 执行 scripts/windows-smoke.ps1（含 -Gui）与 scripts/windows-ipc-audit.ps1，CorePath 均为用户提供路径。

本机日志统一以 `/tmp/xray-pilot-v0.1.1-rc.1-` 为前缀，包含 Linux/Windows 回归、严格 Clippy、release 构建、原生脚本和程序包校验记录。

## 交付及保留边界

发布入口为 [v0.1.1-rc.1](https://github.com/muidea/xray-pilot/releases/tag/v0.1.1-rc.1)，预发布包名为 xray-pilot-0.1.1-rc.1-windows-x64.zip，另附 SHA256SUMS。包内 VERSION.txt 标识版本和完整源码提交。使用方法见 [README](../README.md)，验证建议见 [版本说明](releases/v0.1.1-rc.1.md)。

新增 [打包脚本](../scripts/package-windows.py) 按工作区版本打包三个程序、用户文档、示例和 Windows 验证脚本，逐文件核对内容并生成 SHA-256。CI 构建产物也改为完整 zip 和校验文件。

C01–C16 已关闭各自确认场景；V03 保持受控上游范围关闭。V01、V02、V04–V09 尚缺的目标环境证据以及 F01–F05 的未完成部分不因发布而关闭。Phase 0–2 尚未全部验收。本轮使用模拟代理或 Direct 模式，不修改真实 WinINet、登录任务或用户已有代理进程。
