# 开发与验证

普通用户的安装、配置和部署步骤见 [README](../README.md)。本文用于源码构建、诊断接口和开发验收。

## 从源码构建

以下命令均从仓库根目录运行。使用 Rust 1.94（当前本机验证版本及 CI 配置版本）。Windows 推荐安装 Visual Studio C++ Build Tools 及 Windows SDK，使用 MSVC 工具链；本机也完成了 Windows GNU 构建和原生验证。已发布 rc.3 的远端 MSVC CI 已通过，本文新界面源码须在提交后单独确认远端 CI 结果。

```powershell
cargo build --release --workspace --locked
.\target\release\xray-manager.exe
```

Linux GUI 需要图形桌面；系统代理、托盘、文件选择器和登录自启仅实现 Windows 版本。

当前源码已补配置向导、PAC 自动代理、日志持久化、WinHTTP 只读诊断、Dynamic 依赖和编辑撤销，尚未包含在已发布的 rc.4 中；复测请构建当前源码。流程与边界见 [设计文档核对](phase0-2-design-crosscheck.md)、[本轮收口](phase0-2-remaining-closeout.md) 和 [配置交互实现](configuration-interaction.md)。

```bash
cargo run -p xray-manager-app -- --data-dir /absolute/path/to/data
```

## 高级诊断接口

CLI 命令与源码入口见 [xrayctl.rs](../crates/xray-supervisor/src/bin/xrayctl.rs)。`operation ID` 返回操作记录或大记录摘要；`operation-details ID` 分块还原完整记录。

`request` 从标准输入接收完整的 `xray_model::Request` JSON。写操作须带当前 `expected_generation`，首次应用为 null。请求重试须保留 request ID 和全部原内容；改变内容返回 REQUEST_ID_CONFLICT。

```json
{
  "protocol_version": 1,
  "request_id": "example-create-001",
  "expected_generation": null,
  "command": {
    "operation": "CreateProfile",
    "payload": { "profile_id": "example", "json": "{}" }
  }
}
```

配置原文限制为 4 MiB，JSON 编码后请求还须满足 8 MiB 帧预算。响应超限不表示写操作未执行，应按原 request ID 查询。持久操作记录不因传输摘要而删除。

## 离线维护

`xrayctl --data-dir PATH maintenance` 默认只预览，无需后台。`--retain-days DAYS` 控制孤儿产物及终态压缩的最小年龄，`--apply` 才修改文件；`--compress-operations` 显式启用保留完整语义的 gzip envelope。运行时持有同一 Store 独占锁，存在恢复 / 未完成 / 损坏证据或链接时保守拒绝。压缩格式旧版不可读，降级需完整备份。引用与失败保护合同见 [维护说明](phase0-2-remaining-closeout.md)。

## 回归与平台验收

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked -- --test-threads=4
```

GUI 回归默认不生成截图文件。需要复核真实 egui 图元与字体图集时，将测试进程的 `XRAY_UI_CAPTURE_DIR` 指向 `target/ui-captures` 再运行 `cargo test -p xray-manager-app --locked -- --test-threads=4`；PowerShell 可先设置 `$env:XRAY_UI_CAPTURE_DIR = "target/ui-captures"`，完成后运行 `Remove-Item Env:XRAY_UI_CAPTURE_DIR`。输出的 JSON 网格和 RGBA 字体图集属于本地产物，保存在已忽略的 `target/`，不随源码提交。无窗口图元回归不能替代完整原生桌面交互验收。

PAC 专项不需要官方 Core：`cargo test -p xray-pac` 与 `cargo test -p xray-supervisor --test pac_flow` 覆盖生成、校验、执行测试、不可变发布、租约、端口/版本切换、失败补偿和启动恢复。需要真实 Core 的 PAC 发布用例默认忽略，设置 `XRAY_TEST_CORE` 后运行：

```bash
XRAY_TEST_CORE=/path/to/xray cargo test -p xray-supervisor --test real_core -- --ignored
```

真实 WinINet / PAC 前后可用只读快照脚本 `scripts/windows-proxy-snapshot.ps1 -Save/-Compare` 记录并比较五个当前用户代理字段；脚本不写注册表。会话日志位于数据目录 `logs/supervisor.log`；写入时带 UTC 时间戳和操作/代次上下文，凭据字段脱敏，超过 4 MiB 轮转一个备份，旧备份保留 7 天。日志不是持久审计记录，不应作为唯一恢复凭据。

官方 Core 冒烟测试默认忽略，可选择已有 Core，或运行仅用于开发测试的下载脚本（核对官方 `.dgst` 中 SHA-256）：

```bash
python3 scripts/fetch-test-core.py --destination /tmp/xray-test-core
XRAY_TEST_CORE=/tmp/xray-test-core/xray cargo test -p xray-supervisor --test real_core -- --ignored
```

Windows PowerShell 设置 `$env:XRAY_TEST_CORE = "C:\path\to\xray.exe"` 后执行同一测试命令。测试使用模拟系统代理，不修改本机 WinINet 设置。CI 包含 Linux / Windows 构建、回归和官方 Core 冒烟；Windows 原生 Core、当前用户限定的 Named Pipe、Job 清理及 GUI 关闭/重开已验证；计划任务的创建、外部修改保护及幂等禁用也已验证；真实 WinINet、托盘菜单全部交互、实际登录启动与当前用户权限拒绝仍需进一步验收。程序不注册 Windows 服务或 SYSTEM 级后台任务。

API 内部出站的导入 / 冻结 / 官方 Core 校验专项：

```powershell
$env:XRAY_TEST_CORE = "C:\Tools\Xray\xray.exe"
cargo test -p xray-supervisor --test config_import --locked -- --ignored
```

默认使用合成 API 配置，只导入并校验，不启动 Core。可选设置 `XRAY_TEST_CONFIG` 为待核对的原生配置文件，测试只读该文件，使用独立临时数据目录和模拟代理；断言不输出配置内容和 Core 错误正文，避免扩散认证信息。Linux 同样支持这两个环境变量。CI 显式运行合成配置的官方 Core 专项；常规 `config_import` 回归另外覆盖不完整配置原文保存、请求去重及运行检查拒绝。

在 Windows 上可复测真实 Core、IPC、Job 强制退出清理和 GUI 生命周期（使用隔离数据目录与 Direct 模式）：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\windows-smoke.ps1 -BinaryDir .\target\release -CorePath C:\Tools\Xray\xray.exe -Gui
```

使用解压后的程序包时，将 `-BinaryDir` 改为程序所在目录。脚本文件位于仓库或程序包的 scripts 目录。脚本不会注册登录任务或切换 WinINet 代理。

自启任务专项回归会创建并清理唯一命名的当前用户任务，验证路径、所有权冲突及幂等禁用：

```powershell
cargo test -p windows-integration --lib native_login_task_roundtrip -- --ignored --nocapture
```

本地 A/B 真实流量与 Windows 文件占用回滚专项验证（使用模拟系统代理）：

```powershell
$env:XRAY_TEST_CORE = "C:\Tools\Xray\xray.exe"
cargo test -p xray-supervisor --test acceptance_edges --locked -- --ignored
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\windows-ipc-audit.ps1 -BinaryDir .\target\release -CorePath $env:XRAY_TEST_CORE
```

Linux 设置 `XRAY_TEST_CORE` 后可运行同一 `acceptance_edges` 命令；文件占用用例仅在 Windows 编译。C07–C09 已完成代码收口；说明与剩余验收见 [收口记录](phase0-2-c07-09-closeout.md) 和 [遗留清单](phase0-2-backlog.md)。


当前验收状态以 [遗留清单](phase0-2-backlog.md) 为准。事务、租约、IPC 和恢复合同见 [设计文档](Xray%20Windows%20Manager架构与实现设计.md)；既有修复和复测证据见各轮收口记录。流水线配置不代表远端执行已通过。

交接当前源码、验证证据和剩余验收时，先阅读 [开发交接文档](handoff.md)。

## Windows 程序包

使用 Python 3.11+，在仓库根目录构建并打包：

```powershell
cargo build --release --workspace --locked
python scripts/package-windows.py --binary-dir target/release
```

输出位于 dist，统一使用 `xray-pilot-<版本>-windows-x64.zip`，以及 SHA256SUMS。验证版本通过版本号中的 `rc.N` 区分，例如下一验证版本可使用 `xray-pilot-0.1.1-rc.5-windows-x64.zip`；正式版本例如 `xray-pilot-0.1.1-windows-x64.zip`。文件名不再附加界面名称、验证用途或时间戳。包内包含三个程序、README、版本及源码提交标识、文档、示例和 Windows 验证脚本；不包含 Core 或用户数据。脚本会逐文件核对压缩包内容。正式发布前应使用干净的、与构建输入相同的源码提交，脚本将当前 HEAD 写入 VERSION.txt。

对外提供内容不同的验证包时，应递增预发布版本，同步更新 Cargo.toml 与 Cargo.lock，并重新构建。已发布的版本不复用为不同内容的新包；当前已发布 rc.4，新一轮交付应从 rc.5 起。脚本拒绝覆盖输出目录中的同名包；同版本重复验证构建可使用独立的 `--output-dir`。

仅提供本地验证时，可显式打包尚未提交的当前源码；先构建，再执行：

```powershell
python scripts/package-windows.py --binary-dir target/release --validation
```

`--validation` 使用相同的正式命名规则，附中文验证说明与源码文件指纹清单；验证用途、源码修改状态和构建时间仅记录在 VERSION.txt 与 SOURCE-MANIFEST.json 中。包内 `验证说明.md` 来自 [验证指南](validation-guide.md)，修改验证步骤时维护此文档。SHA256SUMS 汇总同目录中的所有 zip。Windows GNU 交叉构建时将 `--binary-dir` 改为 `target/x86_64-pc-windows-gnu/release`。此模式不提交源码、不创建标签，也不上传发布。

自动依赖部署与完整性/修复验收：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\windows-dependencies-check.ps1
```

此专项从官方获取 Core，在独立临时目录验证安装、修复、离线输入和失败保护，不切换真实系统代理或登录任务。GUI 内嵌同一部署脚本，普通安装不需要开发用 fetch-test-core.py。

Windows 默认数据目录与启动位置一致性验收（在隔离临时安装目录复制程序，不使用现有用户数据）：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\windows-portable-check.ps1 -BinaryDir .\target\release
```

`windows-smoke.ps1 -Gui` 现在通过真实托盘弹出菜单的文本解析菜单 ID，并投递同一原生菜单命令，验证隐藏窗口重开、隐藏/可见窗口的仅退 GUI、后台隐藏启动后的完整退出。运行过程使用 Direct，不改真实 WinINet。

## 程序图标资源

矢量源、PNG 与多尺寸 ICO 位于 `crates/xray-manager-app/assets`。普通构建直接使用已提交资源；Windows 构建会通过 winresource 嵌入 EXE 图标，MSVC 使用 Windows SDK，交叉编译 GNU 使用 MinGW windres。调整图形时安装 ImageMagick，执行 `python scripts/generate-icons.py` 重新生成资源。接入与尺寸说明见 [程序图标](ui-icon.md)。
