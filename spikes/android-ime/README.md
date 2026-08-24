# Android 输入法验证程序

这个一次性程序验证普通多行 `EditText` 能否观察 Android 当前系统输入法产生的完整文本变化。它不联网，不写正文日志，也不申请相机、存储或录音权限。

每次 `TextWatcher.afterTextChanged` 都在内存中记录：

- 完整文本状态和递增序号。
- 新增、删除或替换类型。
- Android UTF-16 变化范围。
- 光标和 composing 区间。
- 从本次启动开始的相对时间。

只有主动点击“复制报告”时，包含测试正文的 JSON Lines 报告才会进入系统剪贴板。

## 构建

```powershell
.\gradlew.bat :app:assembleDebug
.\gradlew.bat :app:connectedDebugAndroidTest
```

APK 输出：

```text
app/build/outputs/apk/debug/app-debug.apk
```

## 实机步骤

1. 安装 APK 并选择任意系统输入法。
2. 依次完成连续语音、识别回改、手工删除替换、标点换行和长文本测试。
3. 每组测试后点击“复制报告”，保存到对应验证记录。
4. 切换输入法、隐藏并重新打开键盘，确认输入框和观察器仍工作。

程序验证 Android 文本框收到的标准编辑状态，不依赖输入法私有接口。

## 模拟器自动验证

`connectedDebugAndroidTest` 通过标准 `InputConnection` 模拟输入法调用，覆盖：

- composing 中间结果被整体回改。
- 尾部删除和中段替换。
- 中文、换行和 emoji 原样保留。
- 5,000 字完整状态不被截断。

模拟器可以验证 Android 标准输入链路；不同真机输入法可能采用不同的组合文本更新节奏。
