# 说写（FlowType）

[English](README.md) | 简体中文

> 手机说，电脑写。

说写把手机输入法里的文字实时同步到 Windows。它直接使用手机当前输入法，语音和键盘输入都可以；Windows 端把文字输入到当前应用。

## 当前版本

当前版本为 `0.2.0`，仍属于 V1 内部发布阶段。V1 的数据路径是：

```text
Android 系统输入法 -> 局域网 WSS -> Windows 当前输入位置
```

同步发送完整文本和递增序号，Windows 根据前后状态计算差异，因此可以处理语音识别中的修正、删除和替换。

## 功能范围

- Android 多行输入框，直接使用手机当前系统输入法
- Android 与多台 Windows 电脑绑定、手工切换或按用户选择的候选电脑自动切换
- 二维码绑定，绑定信息默认长期保存
- WSS 加密传输和设备身份认证
- 自动重连；网络中断时保留最新完整草稿
- Windows 当前光标文本注入，优先覆盖中文 Unicode、VS Code、Codex、浏览器和终端
- Windows 半透明悬浮球：颜色表示连接状态，单击切换手机当前电脑，双击打开主页面，按住拖动位置
- 已绑定电脑保持轻量控制连接，橙色电脑的悬浮球单击也可以通知手机切换；控制连接不传输输入内容
- Android 历史记录、复制、作为新输入和删除
- 可选悬浮输入球和悬浮输入面板
- 单张照片或图库图片发送到 Windows 剪贴板，可选择原图

公网中转、锁屏状态下持续语音输入、多张图片和自动发送 Enter 不在 V1 范围内。

## 安装

### Windows

下载 Release 中的 `FlowType-<version>-x64-setup.exe`，运行安装程序并按提示授予管理员权限。安装程序会注册 FlowType 输入服务、配置开机启动和局域网防火墙规则。

安装后可从开始菜单启动，也可以运行安装目录中的：

```text
flowtype.exe --show
```

### Android

下载 Release 中的 `FlowType-<version>-android-release.apk`，在 Android 设置中允许安装此来源的应用后安装。首次运行时扫描 Windows 应用显示的二维码，随后选择电脑并开始输入。

这是 V1 内测阶段的包名迁移版本：Android applicationId 为 `app.flowtype`。从旧包安装的用户需要先卸载旧版本，再安装新 APK，并重新扫描二维码绑定电脑；旧版本不会与新版本互通。

正式 APK 使用发布签名；本地未配置签名环境时，Gradle 只生成 unsigned 内部构建包。

## 环境要求

### Android 构建

- JDK 17
- Android SDK Platform 36
- Android SDK Build Tools 和 Platform Tools
- Android Gradle Wrapper 8.11.1（仓库已包含）
- Android 最低版本 API 29

### Windows 构建

- Windows 10/11 x64
- Visual Studio Build Tools 的 MSVC C++ 工具链和 Windows SDK
- Rust stable MSVC 工具链
- Inno Setup 6（仅构建安装程序时需要）

## 本地构建

### Android

在仓库根目录执行：

```powershell
cd android
.\gradlew.bat test lint
.\gradlew.bat packageFlowTypeRelease
```

输出位于 `android/app/build/outputs/apk/release/FlowType-<version>-android-release.apk`。如果没有配置签名环境，任务会明确输出 `FlowType-<version>-android-release-unsigned.apk`；该文件只用于内部验证，不能直接作为正式安装包分发。

配置正式签名时需要同时设置以下环境变量：

```text
FLOWTYPE_ANDROID_KEYSTORE
FLOWTYPE_ANDROID_STORE_PASSWORD
FLOWTYPE_ANDROID_KEY_ALIAS
FLOWTYPE_ANDROID_KEY_PASSWORD
```

签名材料不应提交到 Git。

### Windows

```powershell
cd windows
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
rustup target add i686-pc-windows-msvc
cargo build --workspace --release
cargo build -p flowtype-tip --release --target i686-pc-windows-msvc
Copy-Item .\target\i686-pc-windows-msvc\release\flowtype_tip.dll .\target\release\flowtype_tip_x86.dll -Force
```

Release 文件位于 `windows/target/release/`，主要文件为：

```text
flowtype.exe
flowtype-injector.exe
flowtype_tip.dll
flowtype_tip_x86.dll
```

### Windows 安装包

安装 Inno Setup 6 后，在 `windows/` 目录完成 Release 构建，然后从仓库根目录执行：

```powershell
$tipHash = (Get-FileHash .\windows\target\release\flowtype_tip.dll -Algorithm SHA256).Hash.ToLowerInvariant()
$tipX86Hash = (Get-FileHash .\windows\target\release\flowtype_tip_x86.dll -Algorithm SHA256).Hash.ToLowerInvariant()
& "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe" `
  /DBuildDir="..\windows\target\release" `
  /DTipDllHash=$tipHash `
  /DTipDllX86Hash=$tipX86Hash `
  installer/flowtype.iss
```

安装包输出到 `installer/output/`。便携包只需解压后运行 `flowtype.exe`，但不会自动注册输入服务、开机任务或防火墙规则；普通用户应优先使用安装程序。

## 项目结构

```text
android/              Android 应用、会话状态和网络客户端
windows/flowtype-core/ 协议、序号状态机和 Unicode 差异算法
windows/flowtype-app/  Windows 主程序、WSS 服务和 Win32 UI
windows/flowtype-injector/ 高权限输入服务和 TSF 注入
windows/flowtype-tip/ Windows 文本服务组件
protocol/v1/           协议 fixture
docs/                  需求、架构、开发计划和验证记录
installer/             Inno Setup 安装程序脚本
```

UI 层边界见 [客户端 UI 架构](docs/ui-architecture.md)。核心需求和技术决策见 [V1 产品需求基线](docs/requirements-v1.md) 与 [V1 技术方案](docs/architecture-v1.md)。

## 已知限制

- 仅支持同一局域网或可互通的 Tailscale 网络，不提供公网中转
- Windows 注入依赖目标应用当前仍在前台；切换窗口后不会擅自写入新窗口
- 不同输入法和厂商 Android 系统对悬浮窗、后台保活和相机权限的行为可能不同
- 锁屏状态下是否能持续使用语音输入取决于 Android 厂商和输入法，V1 不保证
- Windows 输入服务需要管理员权限；未安装或被安全软件阻止时，手机会显示输入服务不可用
- 当前只支持一次发送一张图片；原图可能较大，受 Windows 接收端大小限制
- 不自动发送 Enter，完成操作不会额外修改目标文本
- GitHub Actions 自动构建、签名清单和 Release 上传已经实现；Windows 与 Android 客户端支持后台检查、可恢复下载、安装前复核和用户确认安装，详见 [在线更新设计](docs/update-design-v1.md)
- 发布前运行 `scripts\verify-version.ps1`，确认 Android、Windows、安装包和文档版本一致

## 安全与隐私

正文通过 TLS 传输，手机和电脑使用设备身份认证；Android 密钥保存在 Keystore，Windows 密钥和绑定数据使用当前用户保护的存储。正文历史只保存在 Android 本地加密存储中，Windows 不建立已完成正文历史。

安全问题请阅读 [SECURITY.md](SECURITY.md)，不要在公开 Issue 中发布密钥、配对二维码、输入正文或网络抓包。

## 参与贡献

请先阅读 [CONTRIBUTING.md](CONTRIBUTING.md)。提交改动前至少运行 Android 测试/Lint 和 Windows Rust 测试/Clippy；协议、会话和注入行为变更应补充测试。

## 许可证

本项目采用 [Apache License 2.0](LICENSE)。第三方依赖仍受各自许可证约束。

## 历史验证记录

`docs/validation/` 中的阶段文档是开发过程的历史记录，不是当前发行版的完整兼容性承诺。早期记录中的 `flowtype-app.exe`、`flowtype-injector.exe` 等文件名是历史构建名称；当前 Windows 文件名以 `flowtype.exe`、`flowtype-injector.exe` 和 `flowtype_tip.dll` 为准。详见 [验证记录说明](docs/validation/README.md)。
