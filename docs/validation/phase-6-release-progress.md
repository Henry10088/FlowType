> 历史记录：本文保留阶段性兼容性与发布验证证据，不代表当前版本的完整兼容性承诺。文中的旧文件名可能已变更，当前名称见 [验证记录说明](README.md)。

# 阶段 6 兼容性与发布进度

日期：2026-08-24

状态：执行中，未达到 V1 发布退出条件。

## 本轮完成

- Android Gradle Plugin 升级到 8.10.1、Gradle 升级到 8.11.1，正式支持 compileSdk 36。
- Android API 36 模拟器完成 15 个 JVM 测试、3 个设备测试、lint（`No issues found`）和 R8 Release 构建；本轮 Debug APK 已重新安装并启动。
- Android Release 支持从环境变量读取正式签名材料；未配置时生成 unsigned 内部包，部分配置时立即失败。
- Windows 移除未使用的 TLS 1.2 编译特性和连接设备名控制台输出。
- Windows 管理界面按已确认亮色原型重做导航、状态、绑定、手机和设置页面，并通过 125% DPI 窗口截图对照。
- Windows 设置页修正原生编辑框垂直对齐、复选框焦点范围和导航点击焦点框，并在真实 Release 窗口中复核。
- 输入助手 IPC 失败改为在现有 WebSocket 内返回 `INJECTOR_UNAVAILABLE`；Android 保留最新全文并显示输入服务错误，不再误判为网络断线而循环重连。
- Windows 网卡选择优先 Tailscale 和实体局域网，避免 Hyper-V/WSL 虚拟交换机进入二维码；当前机器正确选择 `100.0.0.13`。
- TLS 探针验证 TLS 1.2 被拒绝、TLS 1.3 成功协商。
- Windows 11 当前机器完成 13 个 Rust 测试、严格 clippy、Release 构建和无警告 Inno Setup 编译。
- 两个 Windows 程序均验证为 x64、Windows GUI 子系统。
- 仓库跟踪文件未发现 APK、EXE、keystore、私钥或证书材料。
- Android manifest 禁止明文网络和备份，数据提取规则排除云备份及设备迁移。
- Windows 文本同步从按键 diff 重构为目标进程内 TSF 组合范围；Android 每个完整状态原位替换同一范围，不再累计 Backspace 和 Unicode 按键。
- `flowtype_tip.dll` 注册为 `GUID_TFCAT_TIP_SPEECH`；注册过程主动移除旧 `GUID_TFCAT_TIP_KEYBOARD` 子项，FlowType 不出现在键盘输入法列表。
- 真实记事本依次应用 `voice draft 123`、`voice corrected 456`、`TSF 中文最终稿`，最终只保留第三版；一次性启动激活前后的键盘 Profile 和目标线程 HKL 完全一致。
- TIP 使用 `ITfTextEditSink` 区分远程编辑与电脑端编辑；电脑端修改或移动选区后结束远程组合，并向 Android 返回 `TARGET_MODIFIED`。
- 冲突监听改为核对组合区全文和光标位置；TSF 延迟送达的自身编辑事件不会再被误判为电脑端修改，无法读取状态时也不会贸然终止输入。
- 安装脚本已加入 TIP DLL 的注册与卸载注销步骤。当前机器未安装 Inno Setup，尚未编译本轮安装包。
- 真机 `23113RKC6C` 已通过 Tailscale 无线调试更新 Debug APK；更新后设备锁屏，未代替用户解锁执行最终输入复测。
- 排查并修复本地运行版本混装：主程序、注入助手和 COM 注册现统一指向同一 Release 构建。
- Speech Profile 生命周期改为注入助手启动时一次性激活；输入完成、取消、断线和失败不再执行桌面级停用或禁用，避免重置用户输入法。
- TIP 管道从 25 ms 轮询改为可取消的阻塞读取，常驻空闲时不持续唤醒 CPU。
- 真实记事本组合测试和重复启动激活测试通过；助手停止再启动后仍可输入，键盘 Profile 和目标线程 HKL 保持不变。

本机 Android SDK command-line tools 与 Android Studio 的 SDK XML 版本存在环境提示；构建成功且没有 Android lint 错误。发布机器应更新 command-line tools 后再生成正式签名包。

## 内部验证产物

| 产物 | 大小 | SHA-256 |
| --- | ---: | --- |
| `app-release-unsigned.apk` | 836,641 bytes | `7DC7A2817ACF38128AE9E9B6E5BA20F4E984C89C518651A695D578E807DE1A52` |
| `flowtype-app.exe` | 1,729,536 bytes | `745FA7CAEE93B3262E8116ADB1DDADCBD996CF2CD2A3A70BA97F274795E5A678` |
| `flowtype-injector.exe` | 210,944 bytes | `CBB4493472A0EB395F6696A1E80D1967ED8851E2C0CDE5304F01872A3E96BF06` |
| `FlowType-0.1.0-x64-setup.exe` | 2,865,130 bytes | `392175052CFDC5CBDC1DD8272B352A06BA498F3C65DD7746FC6ACB61B6CED277` |

这些哈希只对应当前未签名内部产物；正式签名后必须重新记录。

### TSF 重构内部构建

| 产物 | 大小 | SHA-256 |
| --- | ---: | --- |
| `app-debug.apk` | 7,828,135 bytes | `27D4A32F32DD7B01B80EB8269512C6312BB2423DB0C5BDED95D532627C8E76D6` |
| `flowtype-app.exe` | 2,021,376 bytes | `3641C9E67100581A146C38B77DEB9488518956853C22703218F84C07ABC4CA11` |
| `flowtype-injector.exe` | 256,000 bytes | `CDA416D2F189D688F26E0DC0C8E88601E64178196B34E0C84DF7F667DDFA8B4C` |
| `flowtype_tip.dll` | 216,576 bytes | `DD31BA59DDE52992118451A329936532A41776206C384B64F1E15E739CD748F2` |

Windows 文件来自独立 `target-next` Release 目录，因为旧 TIP 曾被未关闭的记事本进程加载，Windows 不允许原地覆盖 DLL。该目录仅用于本机验证并由 Git 忽略；干净发布环境仍输出到标准 `target/release`。

## 发布阻断项

- 缺少项目所有者提供的 Android 正式 keystore 和 Windows Authenticode 证书；当前所有发布物均未签名。
- 当前进程为 Windows 中等完整性，且 Codex 窗口站不能可靠处理 UAC，因此未执行会修改 Program Files、计划任务和防火墙的安装、覆盖升级、卸载矩阵。
- 真机当前通过 Tailscale 连接，仍需在同一物理局域网验收 mDNS 自动发现、扫码绑定和网络波动恢复。
- TSF 已完成记事本验收，尚未完成 VS Code、Codex、Chrome、Edge、Windows Terminal、PowerShell 和管理员目标兼容矩阵。
- 尚未覆盖 Windows 10 22H2、Android 10 到 15、不同 DPI、横竖屏和字体放大矩阵。

签名步骤见 [V1 发布签名配置](../release-signing.md)。以上任一阻断项未关闭前，不标记 V1 正式发布完成。
