# 说写（FlowType）

[English](README.md) | 简体中文

> **手机说，电脑写。**

FlowType（说写）将你的 Android 手机化身为 Windows 电脑的实时语音输入面板。只需将光标置于 VS Code、Codex、浏览器、终端、文档或即时通讯窗口中，直接在手机端说话或打字，内容即可近乎零延迟地同步键入到 Windows 光标所在位置——彻底告别“手机端输入复制、电脑端切换粘贴”的繁琐流程。

说写直接沿用手机当前系统输入法，无需更换键盘，亦无需在电脑端配置额外的语音识别引擎。

## 为什么需要说写

在长文本与中文输入场景下，手机端语音输入兼具高识别率与极致便捷性。然而，要将输入结果传递到电脑，通常需要在两台设备间经历“复制、切换、粘贴”的多重割裂操作，极易打断沉浸式工作流。

市面上单纯的“按键转发”工具无法应对复杂的语音输入场景：现代语音识别引擎在识别过程中会不断结合上下文回溯修正先前字词、剔除语气词甚至整句重写。若仅流式追加按键，极易导致文字错乱。说写采用**完整文本状态快照与递增序列号**同步机制，Windows 端根据已确认的状态差分计算 Unicode 差异（Diff），使修改、替换与删除操作都能精准无误地映射回原始文本，避免“越输越乱”。

说写的核心优势：

- **无缝沿用手机输入法**：手机上习惯怎样语音输入或打字，在说写中即可完全照常使用，零学习成本。
- **深度适配语音回溯修正**：基于完整快照同步，完美支持语音识别引擎的动态上下文修正与整句重写。
- **光标级直接注入**：文字直接键入目标 Windows 应用光标处，不依赖系统剪贴板，亦无多余中转窗口。
- **局域直连与强加密**：手机与已绑定电脑通过局域网或 Tailscale 虚拟网直连，通信过程默认启用 TLS 1.3 加密，正文无需经由任何公网服务器中转。

## 适用场景

- **AI 助手高效交互**：在 Codex、Claude、ChatGPT 等工具中快速口述结构化长 Prompt。
- **日常文本高效创作**：快速撰写邮件、长篇文档、即时通讯消息、知识库笔记与代码注释。
- **跨平台输入体验统一**：在 Windows 上直接享受 Android 端强大成熟的多语言及方言语音识别能力。
- **站立办公与离机输入**：离开键盘站立办公或会议时，将手机作为手持式无线麦克风与输入扩展面板。
- **多设备无缝流转**：一台手机可在多台已配对的 Windows 电脑之间自由切换输入目标。

## 工作原理

```text
Android 系统输入法
        -> 完整文本快照 + 递增序列号
        -> 局域网 / Tailscale (WSS over TLS 1.3)
        -> Windows TSF 组合文本
        -> 目标前台编辑器与应用
```

1. **配对设备**：在 Windows 端启动说写，使用 Android 客户端扫描界面二维码完成初始安全绑定。
2. **定位光标**：在手机端选择目标电脑，并将 Windows 端光标置于目标输入位置。
3. **即说即显**：在手机端使用熟悉的输入法说话或打字，修改内容近乎实时地同步至 Windows 光标处。
4. **提交归档**：点击“完成”结束本次输入流，内容安全保存于 Android 历史记录中，便于随时查阅与复用。

配对关系持久有效，支持随时手动解除。单台手机可绑定多台电脑；亦可通过 Windows 桌面悬浮控件主动请求手机端切换当前活动电脑。

## 安全与隐私

### 关键安全结论

| 你可能关心的问题 | 结论 |
| --- | --- |
| 局域网或 Wi-Fi 不可信，输入内容还安全吗？ | **安全。** 在二维码确实来自目标电脑、手机和电脑本身未被攻破的前提下，TLS 1.3 会加密正文并检测篡改。 |
| 正确完成绑定后，路由器、网关或同一网络中的其他设备能冒充电脑或手机吗？ | **在设备密钥未泄露的前提下不能。** Android 只接受扫码时固定的 Windows 公钥；Windows 每次连接都要求已绑定手机提供新的签名。中间设备可以阻断连接，但不能在不被发现的情况下读取或修改正文。 |
| 局域网中的设备能看到什么？ | 可以看到 IP、端口、连接时间、大致流量和 mDNS 在线广播，但看不到输入正文。 |
| 输入正文会经过说写的公网服务器吗？ | **不会。** 正文通过局域网或 Tailscale 在手机与所选电脑之间直连；应用只会另外访问 GitHub 检查和下载更新。 |
| 已完成的输入历史保存在哪里？ | 历史在 Android 本地加密保存；说写不会在 Windows 建立已完成正文历史。 |

### 具体保护机制

说写通过扫码建立端到端信任链。本地 mDNS 服务发现仅用于探测已绑定电脑的在线状态，无法用于发起未授权的新绑定，亦无法篡改已保存的地址或固定公钥指纹。

- **传输层强加密**：通信链路全程采用 TLS 1.3，结合扫码固定的 Windows 公钥指纹（Certificate Pinning），确保局域网内的中间人（如路由器、网关）即便能嗅探链路，也无法解密或篡改输入内容。
- **双向设备认证**：二维码内嵌一次性配对令牌；会话重连时采用新的随机挑战，由 Android Keystore 为每台电脑分别创建的私钥签名。密钥是否由硬件保护取决于具体 Android 设备。
- **本地数据与密钥防护**：Android 草稿与历史数据使用 Keystore 中的 AES-GCM 密钥加密；Windows 私钥受系统 DPAPI 保护；负责文本注入的高权限组件不监听网络，也不持有长期绑定密钥。
- **更新完整性校验**：安装前验证独立签名的更新清单、SHA-256 摘要和平台安装包签名，降低更新清单或安装包被篡改后仍被接受的风险。
- **不建立 Windows 正文历史**：说写不会在 Windows 持久化已完成的输入正文，Injector 诊断日志也不记录正文内容。

> [!NOTE]
> **安全边界说明**：上述安全机制无法防御已被攻陷的终端系统、恶意第三方输入法，亦无法限制目标应用程序对其所接收文本的处理行为。更深入的配对流程、密钥生命周期、权限边界与威胁建模分析，请参阅[安全模型文档](docs/security-model.zh-CN.md)（[English](docs/security-model.md)）。如发现安全漏洞，请参阅 [SECURITY.md](SECURITY.md) 提交负责任的私下披露。

## 核心特性

- **多行实时流式输入**：直接调用 Android 原生系统输入法，提供流畅的多行文本同步体验。
- **状态感知与断点续传**：基于完整文本状态机算法，无缝支持实时回溯修正、大段删除、文本替换及网络瞬断恢复。
- **持久配对与多设备管理**：扫码即可完成长期绑定，支持在多台 Windows 电脑之间轻松切换。
- **高可用连接保障**：实时连接状态感知与无缝自动重连，断线期间手机端草稿完好保留不丢失。
- **深度 Windows 原生集成**：基于 Text Services Framework (TSF) 的可替换组合文本，适配 VS Code、Codex、主流浏览器、各类终端及办公软件。
- **轻量桌面悬浮控件**：可拖拽的 Windows 悬浮窗，支持实时状态监控、活动电脑切换与主界面快捷唤起。
- **灵活的移动端悬浮交互**：提供可选的 Android 悬浮球与轻量悬浮输入面板，支持跨应用全局呼出。
- **本地历史记录管理**：Android 端支持输入历史的安全存储、一键复制、重用与清理。
- **图片快速流转**：支持将相机拍摄或图库图片一键推送至 Windows 系统剪贴板（支持原图质量）。
- **原生双语支持**：UI 界面完整支持中文与英文，并自动跟随操作系统语言偏好。
- **可靠的自动更新机制**：支持后台版本检测、断点续传下载、哈希/签名完整性校验及安全更新提示。

## 安装指南

说写目前处于活跃开发的 pre-1.0 阶段，Windows 与 Android 平台使用独立版本号。

### Windows

1. 从 [GitHub Releases](https://github.com/Henry10088/FlowType/releases) 下载 `FlowType-<version>-x64-setup.exe`。
2. 运行安装程序并在 UAC 提示时授予管理员权限（安装程序将自动注册输入组件、配置开机自启及局域网防火墙放行规则）。
3. 安装完成后从开始菜单启动 FlowType，主窗口将显示用于绑定 Android 客户端的二维码。

### Android

1. 从 [GitHub Releases](https://github.com/Henry10088/FlowType/releases) 下载 `FlowType-<version>-android-release.apk`。
2. 在系统设置中允许“安装未知来源应用”，完成 APK 安装。
3. 启动应用并扫描 Windows 端展示的二维码完成配对。

> 系统要求：Android 10（API 29）或更高版本。正式版 APK 附带官方发布签名；本地 unsigned 构建仅供开发调试使用。

## 当前已知限制

- **网络互通要求**：手机与电脑需处于同一局域网或可互通的 Tailscale 虚拟网络中；出于隐私考虑，暂不提供公网中继服务。
- **窗口焦点绑定**：输入会话建立时即锁定当前处于前台的目标窗口；期间若切换焦点，说写不会自动转移注入目标，以防内容误写入。
- **RDP 远程桌面支持**：仅在本地宿主机运行说写无法通过 `mstsc.exe` 直接向远程桌面内部注入文本。需在远程电脑上安装说写，并在手机端直接连接该远程电脑。
- **系统权限与安全拦截**：Windows 端核心输入组件的注册与修复需要管理员权限；部分高安全策略防护软件或沙盒隔离应用可能会拦截底层文本注入。
- **移动端后台与厂商策略**：受各 Android OEM 厂商后台保活策略及特定输入法实现限制，暂无法保证设备完全锁屏状态下的持续语音识别。
- **回车行为控制**：文本内容中的换行符将严格按字符同步；点击“完成”不会触发额外的 Enter（回车提交）按键事件。
- **图片传输规格**：目前单次支持传输一张图片，且仅写入 Windows 剪贴板，不会自动执行粘贴动作。

> 注：公网中转中继、锁屏持续语音识别保证、多图批量传输及自动发送 Enter 不在当前版本规划范围内。

## 项目结构

```text
android/                    Android 客户端应用、会话状态机与网络通信模块
windows/flowtype-core/      核心协议定义、序列号状态机与 Unicode 差分算法
windows/flowtype-app/       Windows 主程序、WSS 服务端与 Win32 UI
windows/flowtype-injector/  高权限文本注入服务与 TSF 协调器
windows/flowtype-tip/       Windows 文本服务框架 (TSF) 核心组件
protocol/v2/                跨语言协议契约与测试 Fixtures
docs/                       产品需求、系统架构、安全模型与开发规划
installer/                  Inno Setup 安装包打包脚本
```

技术资料建议从 [V1 产品需求](docs/requirements-v1.md)、[V1 技术方案](docs/architecture-v1.md)、[UI 架构](docs/ui-architecture.md)以及[协议 v2 约定](protocol/v2/README.md)开始阅读。

<details>
<summary><strong>从源码构建</strong></summary>

### Android 构建环境

- JDK 17
- Android SDK Platform 36、Build Tools 及 Platform Tools
- Android Gradle Wrapper 8.11.1（仓库已内置）

```powershell
cd android
.\gradlew.bat test lint
.\gradlew.bat packageFlowTypeRelease
```

生成带签名的正式 Release APK 需要预先配置以下环境变量：

```text
FLOWTYPE_ANDROID_KEYSTORE
FLOWTYPE_ANDROID_STORE_PASSWORD
FLOWTYPE_ANDROID_KEY_ALIAS
FLOWTYPE_ANDROID_KEY_PASSWORD
```

> **注意**：签名凭据切勿提交至 Git 仓库。

### Windows 构建环境

- Windows 10 / 11 x64
- Visual Studio Build Tools（需包含 MSVC C++ 工具链与 Windows SDK）
- Rust Stable (MSVC 工具链)
- Inno Setup 6（用于打包安装包）

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

生成 Windows Release 二进制文件后，可在仓库根目录下打包安装程序：

```powershell
$tipHash = (Get-FileHash .\windows\target\release\flowtype_tip.dll -Algorithm SHA256).Hash.ToLowerInvariant()
$tipX86Hash = (Get-FileHash .\windows\target\release\flowtype_tip_x86.dll -Algorithm SHA256).Hash.ToLowerInvariant()
& "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe" `
  /DBuildDir="..\windows\target\release" `
  /DTipDllHash=$tipHash `
  /DTipDllX86Hash=$tipX86Hash `
  installer/flowtype.iss
```

发布版本前，请运行 `scripts\verify-version.ps1 -Platform Windows` 或 `-Platform Android`。两个平台使用独立版本号与 GitHub Release。

</details>

## 参与贡献

在提交代码前，请先阅读 [CONTRIBUTING.md](CONTRIBUTING.md)，并确保本地通过 Android 单元测试/Lint 与 Windows 测试/Clippy 检查。

`docs/validation/` 下的开发记录属于阶段性验证证据，不构成对当前发行版的完整兼容性承诺，详情请参阅[验证记录说明](docs/validation/README.md)。

## 开发说明

说写（FlowType）在开发过程中使用 OpenAI Codex 辅助完成代码编写、重构和测试，采用 AI 辅助开发流程。项目维护者负责产品需求、系统架构、实现取舍、代码审查、测试、安全决策和版本发布。

AI 辅助工具不替代工程审查和安全验证。

## 开源许可证

本项目基于 [Apache License 2.0](LICENSE) 协议开源。所引用的第三方依赖组件仍受各自原始许可证约束。
