# C17 收口：Windows 隐藏窗口的托盘命令

日期：2026-11-10，交付版本：v0.1.1-rc.3。

## 复现与原因

v0.1.1-rc.2 的隔离 Windows 实例关闭窗口到托盘后，向该进程的真实 tray_icon_app 窗口投递 Exit GUI / Quit 原生菜单命令，等待 4 秒均没有退出；通过 Windows 重新显示其主窗口后，两者均在 5 秒内完成退出。所有进程与数据均为测试创建的实例。

原实现只在 App::update 中轮询 MenuEvent 队列。隐藏 HWND 后，winit 的普通重绘请求不能继续驱动 GUI 更新，菜单事件因而留在队列中。退出确认还需要后续绘制处理关闭事件；后台 Shutdown 响应也需要唤醒隐藏窗口。

此前 windows-smoke.ps1 用 Kill 结束 GUI，仅证明后台/Core 独立存活，并未覆盖用户从托盘选择退出。这一缺口不能算作托盘菜单验收通过。

## 修复

- 托盘使用 MenuEvent 回调将动作送入 GUI 队列，并通过 Wakeup 异步投递主窗口 WM_PAINT，唤醒隐藏窗口；IPC 响应共用此入口。
- 显式退出在本帧发送关闭命令，并唤醒下一帧完成 eframe 关闭确认；不会被普通窗口关闭到托盘的逻辑取消。退出清除保存的 HWND，后续工作线程不再向已退出的窗口投递。
- 完整退出保留用户意图，等待在途写操作完成，再发送一次 Shutdown；停止新增状态轮询。成功响应之后才退出 GUI。
- Shutdown 失败保留界面与 request ID，并重新显示窗口；结果未知时不会自动重发。
- Exit GUI (keep Core) 继续仅退出界面；Exit and restore proxy 请求 Supervisor 关闭 Core 和恢复代理后退出。

## 数据目录调整

Windows 默认使用可执行文件所在目录的 data 子目录，GUI、独立 Supervisor、CLI 共用，且与当前工作目录无关。随包 scripts/windows-dependencies.ps1 默认使用 scripts 上一级的 data；GUI 内嵌调用显式传递数据目录。保留 --data-dir / -DataDir 优先级。

旧版 LOCALAPPDATA 数据不自动迁移、合并或删除；继续使用旧配置时须显式指定其原目录。安装目录需要可写。Linux 默认目录保持现有 XDG 规则。

## 验证

- Linux 77 项常规回归、Windows 原生 GUI 19 项通过。新增三组状态回归覆盖仅退出界面不写后台、在途写操作之后仅发送一次 Shutdown，以及关闭失败保留界面/请求身份且不盲重试。
- 格式与 Linux/Windows GNU 全目标严格 Clippy 通过；Windows release 构建通过，外部依赖版本未更新。
- 最终 release 使用受管官方 Xray v26.11.9 和隔离 Direct 配置，windows-smoke.ps1 -Gui 通过：关闭隐藏、托盘 Open、隐藏/可见 Exit GUI、后台/Core 继续存活、重开保持实例、Job 强杀清理、历史去重，以及 --background 启动后 Quit 关闭 GUI/Supervisor/Core。系统代理指纹保持不变。
- 冒烟通过真实托盘弹出菜单的文本解析菜单 ID，再投递同一 WM_COMMAND，覆盖原生菜单事件分发；不等同于全部物理鼠标或托盘逐项交互验收。
- windows-portable-check.ps1 复制三个程序与安装脚本到独立临时安装目录，从其他工作目录启动，验证四个入口均使用 installation/data。测试中的 LOCALAPPDATA 也隔离到临时目录，避免错误回退触及现有用户数据。

本轮 Windows 证据目录：xray-pilot-smoke-e024807c44fd4f719b6b9867b5c3609c、xray-pilot-portable-b2ff39a252684a53816d35a0851cfc3d。未修改真实 WinINet 或注册登录任务；V01/V02、其余安全/故障矩阵、V08 的其他 GUI 交互和 F01–F05 未完成部分继续保留。
