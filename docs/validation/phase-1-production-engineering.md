> 历史记录：本文保留阶段性工程验证证据，不代表当前版本的完整兼容性承诺。文中的旧文件名可能已变更，当前名称见 [验证记录说明](README.md)。

# 阶段 1 正式工程验证记录

| 项目 | 结果 |
| --- | --- |
| 日期 | 2026-08-23 |
| 阶段 | 生产工程与核心契约 |
| 结论 | 通过 |

## 实现结果

- 建立原生 Kotlin/XML Android 应用，最低 Android 10（API 29）。
- 建立 Rust workspace，分为核心库、普通权限主程序和注入助手。
- 固化 WSS JSON V1 消息模型、1 MiB 上限、正数 `int64 sequence` 和稳定错误码。
- Android 与 Rust 直接读取同一份 `protocol/v2/valid-messages.json` 契约样例。
- 两端实现完整状态、累计 ACK、完成终态和 Unicode 字素 diff 的核心模型。

## 自动验证

```text
Android: testDebugUnitTest, lintDebug, assembleDebug, assembleRelease 通过
Rust: cargo fmt --check, cargo test --workspace, cargo clippy -D warnings 通过
Rust 单元测试: 8 passed, 0 failed
```

Android Gradle Plugin 8.7.3 对 `compileSdk 36` 输出兼容性建议，但编译、Lint 和打包均通过。当前不隐藏该警告，后续依赖升级时处理。

## 模拟器检查

正式 Debug APK 已安装到可见的 `emulator-5554` 并冷启动。检查结果：

- `MainActivity` 位于前台。
- 原生多行 `EditText` 获得焦点和输入连接。
- 允许物理键盘同时显示软键盘后，系统输入法状态为 `mInputShown=true`。
- 输入页保持屏幕常亮，未绑定状态下完成按钮禁用。

## Release 体积

| 产物 | 字节 |
| --- | ---: |
| Android 未签名 Release APK | 18,341 |
| `flowtype-app.exe` | 108,032 |
| `flowtype-injector.exe` | 108,032 |

体积为阶段基线，不包含以后引入的 TLS、二维码、相机和 Windows UI 依赖。
