# FlowType 0.1.13 发布验证

日期：2026-08-24

## 变更范围

- Windows x64、x86 Release 全部静态链接 MSVC CRT，Windows IoT 不再要求 `VCRUNTIME140.dll`。
- 安装程序继续原地升级，不删除绑定、历史、身份或输入服务注册。

## 自动验证

- Rust workspace tests：29 通过，2 个交互桌面集成测试按设计忽略。
- Rust clippy：`-D warnings` 通过。
- Android `testDebugUnitTest`、`lintDebug`：通过。
- Windows x64 Release 与 x86 TIP Release：通过。
- PE 依赖检查：主程序、注入服务、x64/x86 TIP 均无 VC++ CRT DLL 依赖。
- Windows 主程序、注入服务、x64/x86 TIP 和安装包：Authenticode 状态均为 `Valid`。
- Android Release：APK Signature Scheme v2 验证通过。

## 发布产物

- `FlowType-0.1.13-x64-setup.exe`
  - SHA-256: `11F283619660CA4CDDF0C51F1345C3EC10C2E4ED027CB4BBAA244E749E0FDC9E`
- `FlowType-0.1.13-android-release.apk`
  - SHA-256: `0A1AC8ED3B704E7381F14939A18F3127990F0D0FF18A23A2726746C7F116A1A3`
