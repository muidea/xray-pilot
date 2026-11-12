# 代码评审记录（2026-11-12）

范围：当前工作区相对 `efe1fcf` 的未提交修改，重点为 PAC、Dynamic 依赖、Mixed 就绪、持久化日志、WinHTTP 诊断和 Windows 验证包。评审结论：**未发现阻断打包的缺陷**；以下高置信问题已在打包前修复并加入回归。

## 已修复

| 编号 | 严重度 | 问题 | 修复 |
| --- | --- | --- | --- |
| R01 | 重要 | PAC 目标同时设置 `PROXY_TYPE_DIRECT`，Supervisor 被强杀后 PAC 服务不可达时可能回退直连 | PAC 目标改为仅 `AUTO_PROXY_URL`，不设隐式 DIRECT；验证包说明残留的 WinINet 客户端行为仍需实测 |
| R04 | 重要 | 托盘/总览切换 PAC 时使用“正在编辑配置”的 PAC 草稿，而运行事务作用于当前运行配置 | `SetProxyMode` 改用运行配置的 PAC 草稿；新增编辑 B、运行 A 的回归 |
| R05 | 重要 | Core `-test` 诊断按 1024 字节分块后分别脱敏，跨块凭据会泄漏；`Authorization` 与 `key = "value"` 未覆盖 | 先合并有界诊断再脱敏；增加 `authorization`、空格键值和转义值处理；Core 运行日志按行缓冲后脱敏 |
| R07 | 次要 | TCP 延迟探测成功结果未写入 GUI | 增加 `TestLatency` 响应处理和结果展示回归 |
| R08 | 次要 | PAC `shExpMatch` 为递归实现，恶意通配模式可能耗尽栈/CPU | 改为迭代匹配并限制 1024 字节输入 |
| R09 | 次要 | Boa 可接受 ES2020 语法，但 Windows WinINet JScript 可能拒绝 | 语法检查增加 `let`/`const`/箭头函数/模板字符串等兼容性提示 |
| R10 | 次要 | `WinHttpGetIEProxyConfigForCurrentUser` 读取的是当前用户 WinINet/IE 配置，却被标为机器级 WinHTTP | 改用 `WinHttpOpen` + `WinHttpQueryOption(WINHTTP_OPTION_PROXY)` 读取 WinHTTP 会话代理，界面与模型字段同步 |
| R11 | 重要 | 更新配置新增/改名 HTTP 入站后，PAC 页仍保留旧 `http_tag`，持续提示“请先添加回环、无认证入口” | `pac_endpoint` 每次从当前编辑器重新枚举入站；优先当前标签，失效时自动选择第一个可用入口，并在全部不可用时显示具体原因；新增配置更新回归 |

统一域名策略编辑器及编译器在评审后按“简化不必要的配置和管理能力”的要求从产品界面移除；上述 R02/R03/R06 不再适用。保留原生路由冲突提示。

## 复核

- 全工作区 **208 项**常规回归通过，7 项需显式环境用例忽略。
- Linux 与 Windows GNU 全目标严格 Clippy、`cargo fmt --check` 通过。
- Windows GNU `--release --workspace` 构建通过，三个 EXE 均生成。
- PAC 端口/版本生命周期、旧发布保留、补偿恢复、Maintenance 引用/保留和 Mixed 就绪未发现新的高置信问题。
- 未执行真实 WinINet / PAC URL、Windows 原生桌面、官方 Core 和远端 CI；这些仍属于 V01 / V02 / V04–V10 的验证范围。

## 残留风险

- PAC 服务仍由 Supervisor 进程持有；强制结束后 PAC URL 不可达。已去掉隐式 DIRECT 位，但 WinINet 的失败回退行为必须在 Windows 上验证。
- PAC 自定义脚本的完整 JScript 兼容、内存隔离和双向原生规则合并仍未实现；详见 [遗留清单](phase0-2-backlog.md)。
