# FlowType 0.1.14 发布验证

日期：2026-08-25

## 变更范围

- Windows 配对地址优先选择真实 Ethernet/Wi-Fi 地址，ReachOps 虚拟地址保留为候选地址。
- 发布版本统一递增到 `0.1.14`；Android `versionCode=15`。
- 版本检查脚本会阻止未递增版本的可分发构建。

## 自动验证

- Rust workspace tests：通过，2 个交互桌面集成测试按设计忽略。
- Rust clippy：`-D warnings` 通过。
- Rust 格式检查：通过。
- Android `testDebugUnitTest`、`lintDebug`：通过。
- Android Release 构建：通过，当前未配置签名材料，因此为未签名内部测试包。
- Inno Setup 6.7.3：编译通过。
- 版本一致性检查：通过，`0.1.14` / `versionCode=15`。

## 发布产物

- `FlowType-0.1.14-x64-setup.exe`
  - SHA-256: `537D27512E5C0FF7774C376C37CAA5965B9266786F9702B55DB846783CD4F272`
- `FlowType-0.1.14-android-release.apk`（未签名内部测试包）
  - SHA-256: `E4ECE7007927EDFCA77EF991CBFA1CE66B72D7DC3B84D8C5BF72B1DAC6F0F52E`
