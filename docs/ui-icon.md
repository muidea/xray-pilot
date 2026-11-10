# 程序图标

采用浅色 X 与青绿色连接线，右上端点表示连接方向；深青色圆角底和描边保证图标在浅色、深色桌面上均可辨识。图形不使用小字、阴影或复杂细节，兼顾 Windows 托盘的小尺寸显示。

![Xray Pilot 图标](../crates/xray-manager-app/assets/xray-pilot.png)

同一矢量源用于三个位置：

| 位置 | 资源与接入 |
| --- | --- |
| 托盘 | 32 px PNG，`tray.rs` 从共享图标模块读取，替换原来运行时绘制的蓝底叉号 |
| 窗口标题栏、运行中的任务栏 | 256 px PNG，经 `ViewportBuilder::with_icon` 设置标题图标，另显式设置 Windows 大图标供任务栏使用 |
| EXE、资源管理器与快捷方式 | ICO 包含 16 / 20 / 24 / 32 / 40 / 48 / 64 / 128 / 256 px，构建时嵌入 Windows 资源 |

矢量源为 `crates/xray-manager-app/assets/xray-pilot.svg`；PNG 与 ICO 一并纳入源码，普通构建和运行不需要图形工具。开发时安装 ImageMagick 后执行 `python scripts/generate-icons.py`，即可从 SVG 重新生成资源。Windows GNU 与 MSVC 构建通过 `build.rs` 的 winresource 资源编译流程接入。

此次同时清理界面冗余说明：移除侧栏“本机连接管理”“本机工作空间”和数据存储说明，以及顶部后台连接标签；没有操作反馈时隐藏底部消息栏。断连、恢复异常、保存失败及结果待确认仍由对应提示呈现，连接事件保留在日志中。

Windows 验证已提取 EXE 的图标资源，并在隔离 GUI 完成初始化后读取标题栏 / 任务栏的两个原生图标句柄，确认均非空且图形一致。托盘读取同一矢量源生成的 32 px 图标。验证记录见 [本轮收口](phase0-2-c18-20-closeout.md)。
