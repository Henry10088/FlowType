# FlowType 0.1.12 发布验证

日期：2026-08-24

## 变更范围

- Windows 继续监听 `0.0.0.0:32187`，同时接受 Tailscale 和普通局域网连接。
- 配对二维码增加多个地址候选，并保留单地址字段兼容旧版 Android。
- Android 连接失败时顺序尝试候选地址，成功地址保存为下次首选。
- 连接成功时 Windows 下发最新地址候选，旧绑定无需重新扫码。
- 已知 Hyper-V、WSL 和 Default Switch 地址不进入二维码候选列表。
- Android 数据库从版本 2 原地升级到版本 3，不删除绑定、历史和密钥。

## 自动验证

- Rust workspace tests：29 通过，2 个交互桌面集成测试按设计忽略。
- Rust clippy：`-D warnings` 通过。
- Android `testDebugUnitTest`：通过。
- Android `lintDebug`：通过。
- Windows x64 Release 与 x86 TIP Release：通过。
- Android Release：APK Signature Scheme v2 验证通过。
- Windows 主程序、注入服务、x64/x86 TIP 和安装包：Authenticode 状态均为 `Valid`。

## 发布产物

- `FlowType-0.1.12-x64-setup.exe`
  - SHA-256: `4B83BF1B9DCB5FDE7676983341E48C759BB354F3308B5FD0927D6B1D8DA60F13`
- `FlowType-0.1.12-android-release.apk`
  - SHA-256: `C9C5F62E1FC2C7CAF8FEE280575F8BA56A51F95C90F58D9089CFF9D24090D561`
  - 签名证书 SHA-256: `F50A154471F500CB5550CAF730EF78F179A410A7B8C2D59BEF3370262A02D0DA`
