> 历史记录：本文保留阶段性可靠性与安全验证证据，不代表当前版本的完整兼容性承诺。文中的旧文件名可能已变更，当前名称见 [验证记录说明](README.md)。

# 阶段 3 一致性与安全验证记录

| 项目 | 结果 |
| --- | --- |
| 日期 | 2026-08-23 |
| 阶段 | 一致性与安全 |
| 结论 | 通过 |

## 一致性

- Android 发送队列最多保留一个等待 ACK 状态和一个最新待发送状态。
- ACK 是累计确认；旧 ACK 不回退，最终 ACK 清空队列并冻结会话。
- 断线后以 250 ms 起步指数退避，最高 10 秒；重连发送最新完整状态和最后 ACK。
- 当前实现使用 `250 ms -> 1 s -> 2 s -> 5 s -> 10 s` 正文重连退避、2 秒 TCP 超时和 4 秒 `ready` 总时限；文本变化可立即结束退避。
- 二维码候选只在首次配对时尝试；首个成功地址固定保存，mDNS 只更新在线状态。
- 空闲 30 秒后的首次编辑在能力协商成功时并行发送轻量健康检查；1.5 秒无任何服务端响应即废弃旧 socket，并以 `resume + 最新全文` 恢复。
- 目标失焦时保留状态并定时重试，不跟随到新窗口。
- 目标关闭或 Windows 会话丢失后停止自动注入，Android 保留全文。
- Windows 离线时产生的新草稿恢复连接后不会自动锁定前台窗口，必须点击“同步全文”。
- 未完成草稿在 200 ms 窗口内合并保存，后台和完成时同步提交。

队列单元测试覆盖慢 ACK 合并、最新状态覆盖、断线恢复、最终 ACK、离线草稿和进程恢复语义。

## 身份与本地数据

- Android 为每台电脑生成独立 Keystore P-256 签名密钥。
- 每次 WSS 连接由 Windows 生成随机 nonce，Android 对域分隔后的电脑 ID、手机 ID 和 nonce 签名。
- Windows 持久化手机公钥并验证 ECDSA SHA-256 DER 签名。
- Windows TLS 私钥使用当前用户 DPAPI 保护；阶段 2 明文身份文件会原地迁移。
- Android 草稿使用 Keystore AES-GCM 加密后写入应用私有偏好文件。
- 绑定成功后 Android 立即删除一次性配对令牌。

## 权限边界

- 命名管道 DACL 只允许对象所有者、Administrators 和 SYSTEM。
- 注入助手通过管道客户端 PID 查询主程序完整路径，只接受同目录 `flowtype-app.exe`。
- 目标窗口同时锁定 HWND 和进程 ID，避免句柄复用后注入其他进程。
- `SendInput` 部分成功或结果未知时立即丢弃助手会话，不自动重试。
- 网络进程不调用 `SendInput`；注入助手不监听网络、不保存正文和绑定密钥。

## 模拟器与本机验证

| 用例 | 结果 |
| --- | --- |
| 首次二维码 + Keystore 公钥注册 | 通过 |
| Android 进程重启后无令牌挑战登录 | 通过 |
| Windows 主程序重启后自动重连 | 通过 |
| 同一二维码第二次注册 | 拒绝，显示“电脑绑定已失效，请重新扫描” |
| DPAPI 文件迁移 | 通过；明文 `key_der` 为空，受保护字段存在 |
| 草稿文件搜索正文 `draft123` | 未发现明文，AES-GCM 密文字段存在 |
| Android 进程重启恢复 `draft123` | 通过 |
| 离线草稿恢复连接 | 保留正文并等待用户点击“同步全文” |
| 新认证连接替代旧连接 | 通过；旧连接后续业务被拒绝 |

## 自动检查与体积

```text
Android: testDebugUnitTest, lintDebug, assembleRelease 通过
Rust: cargo fmt --check, cargo test --workspace, cargo clippy -D warnings 通过
Rust: 11 tests passed, 0 failed
```

| 产物 | 字节 | 预算 |
| --- | ---: | ---: |
| Android 未签名 Release APK | 551,724 | 10 MiB |
| `flowtype-app.exe` | 1,484,800 | 8 MiB |
| `flowtype-injector.exe` | 210,944 | 1 MiB |

Android 构建仍提示 ZXing 旧式 Activity 回调弃用警告，不影响构建和扫码；在阶段 4 页面重构时切换到 Activity Result API。
