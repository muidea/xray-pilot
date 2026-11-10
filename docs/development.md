# 开发与验证

普通用户的安装、配置和部署步骤见 [README](../README.md)。本文用于源码构建、诊断接口和开发验收。

## 从源码构建

以下命令均从仓库根目录运行。使用 Rust 1.94（当前本机验证版本及 CI 配置版本）。Windows 推荐安装 Visual Studio C++ Build Tools 及 Windows SDK，使用 MSVC 工具链；本机也完成了 Windows GNU 构建和原生验证，远端 MSVC CI 尚待验收。

```powershell
cargo build --release --workspace --locked
.\target\release\xray-manager.exe
```

Linux GUI 需要图形桌面；系统代理、托盘、文件选择器和登录自启仅实现 Windows 版本。

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

## 回归与平台验收

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked -- --test-threads=4
```

官方 Core 冒烟测试默认忽略，可选择已有 Core，或运行仅用于开发测试的下载脚本（核对官方 `.dgst` 中 SHA-256）：

```bash
python3 scripts/fetch-test-core.py --destination /tmp/xray-test-core
XRAY_TEST_CORE=/tmp/xray-test-core/xray cargo test -p xray-supervisor --test real_core -- --ignored
```

Windows PowerShell 设置 `$env:XRAY_TEST_CORE = "C:\path\to\xray.exe"` 后执行同一测试命令。测试使用模拟系统代理，不修改本机 WinINet 设置。CI 包含 Linux / Windows 构建、回归和官方 Core 冒烟；Windows 原生 Core、Named Pipe、Job 清理及 GUI 关闭/重开已验证；计划任务的创建、外部修改保护及幂等禁用也已验证；真实 WinINet、托盘菜单全部交互、实际登录启动与权限拒绝仍需进一步验收。

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

## Windows 程序包

使用 Python 3.11+，在仓库根目录构建并打包：

```powershell
cargo build --release --workspace --locked
python scripts/package-windows.py --binary-dir target/release
```

输出位于 dist：版本命名的 Windows x64 zip 以及 SHA256SUMS。包内包含三个程序、README、版本及源码提交标识、文档、示例和 Windows 验证脚本；不包含 Core 或用户数据。脚本会逐文件核对压缩包内容。正式发布前应使用干净的、与构建输入相同的源码提交，脚本将当前 HEAD 写入 VERSION.txt。

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
