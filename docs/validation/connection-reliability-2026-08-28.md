# 连接可靠性与流量优化验证记录

| 项目 | 结果 |
| --- | --- |
| 日期 | 2026-08-28 |
| 范围 | 固定地址、连接超时、重连、空闲健康确认、多电脑控制连接 |
| 自动验证 | 通过 |
| 真机弱网验证 | 待执行 |

## 已实现

- 二维码候选地址只在首次配对时轮询；第一个完整收到 `ready` 的地址永久保存，其他地址丢弃。
- mDNS 只更新在线状态，不再改写绑定 IP 或触发地址切换。
- 正文连接使用 2 秒 TCP 超时、4 秒 `ready` 总时限和 `250 ms -> 1 s -> 2 s -> 5 s -> 10 s` 退避。
- 握手截止、正文重连、目标重试和健康确认使用独立任务；generation 忽略旧 socket 的迟到回调。
- 退避期间发生文本变化会立即连接；4 秒内仍在握手时只缓存最新完整文本。
- 空闲 30 秒后首次编辑在能力协商成功时发送 `health_check`，正文不等待健康 ACK；1.5 秒无任何服务端响应时立即重连。
- 重连保留会话、序号、单条 in-flight 和最新 pending，恢复后发送 `resume + 最新完整文本`。
- 当前电脑只保留正文 WSS，并由它接收悬浮球切换命令；非当前电脑才保留控制 WSS。
- Windows 切换请求使用随机请求号；新 Android 返回 `switch_ack`，Windows 等待 2.5 秒后才判定手机未响应。
- `ready.capabilities` 和认证能力列表保证新增健康检查与切换确认不会破坏旧 v1 对端。

## 流量边界

- 正文仍为完整文本状态，不增加防抖、压缩、Android diff 或额外正文签名。
- 正文队列仍只有一个等待 ACK 的状态和一个最新待发送状态，中间状态可被覆盖。
- 当前电脑继续使用 15 秒 WebSocket Ping/Pong，优先保证闲置后首次输入体验。
- 删除当前电脑重复控制连接；非当前离线控制连接最高退避到 60 秒。
- 打开 Android 主应用或显示悬浮球时立即唤醒控制连接，不以节省后台重试牺牲明确的前台操作。

## 自动验证

```text
Android: testDebugUnitTest, lintDebug, assembleDebug, assembleRelease 通过
Android emulator-5554: 覆盖安装成功，MainActivity 冷启动到 RESUMED，AndroidRuntime 无崩溃
Windows: cargo fmt --check 通过
Windows: cargo test --workspace 通过，48 passed，2 个交互测试按设计 ignored
Windows: cargo clippy --workspace --all-targets -- -D warnings 通过
Windows: cargo build --workspace --release 通过（使用独立验证 target，未中断正在运行的客户端）
```

## 真机待验收

1. 已连接后闲置超过 30 秒，首次语音输入立即同步，不出现页面闪烁或长时间“连接中”。
2. 输入过程中断开并恢复 Wi-Fi/Tailscale，手机继续编辑，恢复后 Windows 最终与手机完整文本一致。
3. 连接黑洞地址时约 4 秒结束单次尝试，且不存在两个 socket 或两个重连任务并行。
4. 固定 IP 不可达时只重试该 IP；mDNS 发现同一电脑的新 IP 时不改写绑定，需要重新添加电脑。
5. 多电脑在线、离线和恢复场景下，当前电脑没有重复控制 WSS；Windows 悬浮球仅在收到 `switch_ack` 后显示成功。
6. Android 进程休眠或被系统停止时，Windows 悬浮球明确显示手机未响应，不声称可以点亮屏幕或唤醒应用。
