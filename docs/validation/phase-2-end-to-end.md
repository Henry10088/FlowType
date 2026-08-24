> 历史记录：本文保留阶段性端到端验证证据，不代表当前版本的完整兼容性承诺。文中的旧文件名可能已变更，当前名称见 [验证记录说明](README.md)。

# 阶段 2 最小闭环验证记录

| 项目 | 结果 |
| --- | --- |
| 日期 | 2026-08-23 |
| 阶段 | 最小端到端闭环 |
| 结论 | 内部构建完成 |

## 实现结果

- Windows 首次运行生成 P-256 电脑身份、自签名证书、一次性配对令牌和二维码。
- Windows 只接受 TLS 1.3 WSS，二维码携带 SPKI SHA-256 指纹。
- Android 使用二维码指纹建立严格固定身份的 OkHttp WSS 连接，不信任其他自签名证书。
- Android 扫码组件、正式深链解析、绑定保存、自动进入输入页和系统输入法焦点已接通。
- 原生多行 `EditText` 每次变化发送递增序号和完整正文。
- Windows 普通进程只解析有界类型化消息，通过命名管道调用独立注入助手。
- 注入助手锁定第一条状态到达时的前台窗口，按 Unicode 字素 diff，使用 `KEYEVENTF_UNICODE` 和 Enter 注入。
- 只有助手成功返回后，Windows 才发送累计 ACK；最终 ACK 会冻结并清空 Android 当前输入。

## 自动验证

```text
Android: testDebugUnitTest, lintDebug, assembleRelease 通过
Rust: cargo fmt --check, cargo test --workspace, cargo clippy -D warnings 通过
Rust 单元测试: 9 passed, 0 failed
```

## 模拟器与本机联调

在 API 36 可见模拟器上执行了以下正式链路：

1. 清空应用数据并安装正式 Debug APK。
2. 使用 Windows 生成的二维码 URI 触发与扫码结果相同的正式解析入口。
3. 通过 `adb reverse` 连接 Windows 的 TLS 1.3 WSS。
4. Android 验证电脑 SPKI，Windows 消耗一次性令牌并保存手机 ID。
5. Android 显示 `已连接到：HUAAO`，输入框获得焦点。
6. 新增、删除、替换和换行形成完整状态，并逐条完成 WSS 到命名管道的往返。

联调过程中发现并修复了命名管道消息模式与长度前缀读取冲突。正式实现统一使用字节流管道和 1 MiB 长度上限。

当前命令执行环境所在窗口站无法取得交互桌面的前台窗口，`GetForegroundWindow()` 返回空句柄；`computer-use` 也因桌面光标访问被拒绝而无法代替用户聚焦。因此本轮没有伪造真实目标窗口的组合结果或 P95。底层标准 Win32 Edit 控件的 Unicode、连续回改和多行 `SendInput` 已在 [早期输入与注入验证记录](phase-1-core-validation.md) 中真实通过；交互桌面的整链路延迟和目标应用组合测试在阶段 6 重新记录。

## Release 体积

| 产物 | 字节 | 预算 |
| --- | ---: | ---: |
| Android 未签名 Release APK | 540,588 | 10 MiB |
| `flowtype-app.exe` | 1,441,792 | 8 MiB |
| `flowtype-injector.exe` | 202,240 | 1 MiB |

二维码、扫码、OkHttp、TLS、WSS 和注入依赖加入后，三个产物均低于阶段预算。
