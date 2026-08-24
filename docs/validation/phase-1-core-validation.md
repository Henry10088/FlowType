> 历史记录：本文保留阶段性技术验证证据，不代表当前版本的完整兼容性承诺。文中的旧文件名可能已变更，当前名称见 [验证记录说明](README.md)。

# 阶段 1 核心技术验证记录

| 项目 | 内容 |
| --- | --- |
| 日期 | 2026-08-23 |
| 状态 | Android 标准输入链路和 Win32 基础注入验证完成 |
| Android 验证程序 | `spikes/android-ime` |
| Windows 验证程序 | `spikes/windows-inject` |

## 1. 验证环境

- Windows 11 专业版，64 位，版本 `10.0.26200`。
- Rust `1.94.1`，MSVC 工具链。
- OpenJDK `17.0.10`。
- Android SDK Platform 36，ADB `37.0.0`。
- Android 16 / API 36 模拟器 `Learn_API_36`，Google APIs x86_64，Gboard。
- 当前没有真实 Android 设备连接。

## 2. Android 输入法验证程序

验证程序只包含原生多行 `EditText` 和本地事件观察器，不声明网络、录音、相机或存储权限。

每次 `TextWatcher.afterTextChanged` 在内存中记录：

- 递增序号和完整文本状态。
- 新增、删除或替换类型。
- UTF-16 变化范围。
- 选择区和 composing 区间。
- 相对时间。

包含正文的 JSON Lines 只会在用户主动点击“复制报告”后进入系统剪贴板。

### 2.1 自动结果

| 项目 | 结果 |
| --- | --- |
| `ChangeKind` 单元测试 | 通过 |
| API 36 `InputConnection` 仪器测试 | 4 项通过，0 failure，0 error，0 skipped |
| Android Lint | 通过，0 issue |
| Debug APK | 构建成功，2,401,494 字节 |
| R8 Release APK | 构建成功，未签名，46,357 字节 |

可安装 Debug APK：

```text
spikes/android-ime/app/build/outputs/apk/debug/app-debug.apk
```

### 2.2 API 36 模拟器结果

| 项目 | 结果 |
| --- | --- |
| Debug APK 安装和冷启动 | 通过 |
| Activity 成为前台 | 通过 |
| 多行 `EditText` 自动获得焦点 | 通过 |
| 系统 Gboard 输入窗口自动显示 | 通过，`mInputShown=true` |
| 系统按键新增和连续删除 | 通过；15 个事件严格递增，最终文本为 `FlowType145` |
| composing 中间结果整体回改 | 通过 |
| 尾部删除和中段替换 | 通过 |
| 中文、换行和 emoji 原样保留 | 通过 |
| 5,000 字完整状态 | 通过，无截断 |

仪器测试通过真实 `InputConnection` 调用 `setComposingText`、`finishComposingText`、`commitText`、`deleteSurroundingText` 和 `setSelection`，验证 Android 标准输入链路的行为。不同系统输入法可能使用不同调用顺序，但最终都通过标准文本编辑接口更新 `Editable`。

### 2.3 普通系统输入法真机矩阵

以下项目可在完整产品阶段使用任意系统输入法做真机抽查，不再作为独立前置阶段：

| 用例 | 操作 | 通过条件 | 结果 |
| --- | --- | --- | --- |
| 连续语音 | 连续说话至少 60 秒 | 所有可见中间文本变化都产生递增完整状态，最终状态与输入框一致 | 待测 |
| 识别回改 | 使用当前输入法产生一次中段修正 | 观察器记录替换或删除后新增，最终状态无旧词残留 | 非前置门槛 |
| 手工删除替换 | 删除中间和尾部文字，再插入新文字 | 完整状态准确，变化范围合理 | 待测 |
| 标点 | 使用语音标点和手工标点 | 中文、英文标点原样保留 | 待测 |
| 换行 | 在多行输入框中输入多个换行 | 完整状态包含原始 `\n`，不替换为空格 | 待测 |
| 长文本 | 连续输入至少 5,000 个字符 | 无明显丢事件、卡顿或崩溃 | 待测 |
| 隐藏键盘 | 隐藏后重新唤起当前输入法 | 输入框内容不丢失，后续变化继续记录 | 非前置门槛 |
| 切换输入法 | 在两个系统输入法之间切换 | 观察器继续工作，不依赖输入法私有 API | 非前置门槛 |

出现可稳定复现的标准输入问题时，直接在正式 Android 实现中修正并补充自动测试。

## 3. Windows 注入验证程序

验证程序根据两次完整文本状态按 Unicode 字素计算最长公共前缀，向旧尾部发送 Backspace，再通过 `KEYEVENTF_UNICODE` 注入新尾部。换行单独作为 Enter 注入。

### 3.1 自动结果

| 项目 | 结果 |
| --- | --- |
| `cargo fmt --check` | 通过 |
| `cargo clippy --all-targets -- -D warnings` | 通过 |
| Unicode 字素 diff 单元测试 | 5 项通过 |
| Win32 标准多行 Edit 控件真实 `SendInput` 自检 | Unicode、连续回改、多行全部精确通过 |
| Release EXE | 构建成功，232,960 字节 |

自检不是直接修改控件内容：程序创建真实 Win32 编辑控件、保持标准消息循环、通过 `SendInput` 发送按键，再读取控件正文逐步精确比对。

Release 程序：

```text
spikes/windows-inject/target/release/flowtype-windows-inject-spike.exe
```

### 3.2 安全行为

- 倒计时结束时锁定前台顶层窗口句柄。
- 每次状态注入前验证同一窗口仍在前台。
- 目标失焦时立即退出，不向新窗口跟随注入。
- 控制台只输出长度和 diff 动作，不记录目标正文。
- 普通进程向管理员窗口注入预计会被 UIPI 阻止；管理员矩阵必须从已提升进程单独运行。

### 3.3 目标应用矩阵

每个目标使用新的空白文档或无副作用输入区，依次执行 `unicode` 和 `rewrite`。`multiline` 会发送 Enter，不得在会执行命令或提交表单的输入区运行。

| 目标 | 普通权限 | 管理员权限 | Unicode | 回改 | 多行 | 结果 |
| --- | --- | --- | --- | --- | --- | --- |
| Win32 标准 Edit | 已测 | 不适用 | 通过 | 通过 | 通过 | 自动自检通过 |
| Visual Studio Code | 待测 | 待测 | 待测 | 待测 | 待测 | 待测 |
| Codex | 待测 | 待测 | 待测 | 待测 | 待测 | 待测 |
| Chrome | 待测 | 待测 | 待测 | 待测 | 待测 | 待测 |
| Edge | 待测 | 待测 | 待测 | 待测 | 待测 | 待测 |
| Windows Terminal | 待测 | 待测 | 待测 | 待测 | 不自动测试 | 待测 |
| PowerShell | 待测 | 待测 | 待测 | 待测 | 不自动测试 | 待测 |

阶段 1.2 只有在核心目标矩阵完成并记录已知限制后才能退出。

## 4. 当前结论

- Android 和 Windows 验证程序均可重复构建，自动检查通过。
- API 36 模拟器已经证明普通多行 `EditText` 可以观察标准 `InputConnection` 的 composing 回改、删除、替换、多行、emoji 和 5,000 字完整状态。
- 标准 Win32 控件已经证明中文、英文、标点、emoji、组合字符、尾部回改和 Enter 的底层注入路径成立。
- 不针对任何输入法的私有回调顺序提供保证；产品只依赖 Android 标准文本编辑结果。
- 尚不能证明 Electron、Chromium 和终端类目标具有同样行为。
- 因此阶段 1 仍为进行中，不进入 WSS 与正式业务实现。

## 5. 复现命令

Android：

```powershell
cd spikes/android-ime
.\gradlew.bat :app:testDebugUnitTest :app:lintDebug :app:connectedDebugAndroidTest :app:assembleRelease
```

Windows：

```powershell
cd spikes/windows-inject
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
.\target\release\flowtype-windows-inject-spike.exe --self-test
```
