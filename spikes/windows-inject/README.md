# Windows 文本注入验证程序

这是一个无网络的最小 Rust 程序，用来验证说写的 Unicode 注入、尾部回改、换行和目标窗口锁定。它不是正式注入助手。

## 安全边界

- 启动后留出倒计时，让测试者把光标放到目标窗口。
- 倒计时结束时锁定前台窗口句柄。
- 每次完整状态注入前再次检查前台窗口；一旦失焦立即停止，不跟随到新窗口。
- 不读取目标窗口正文，不写入输入内容日志。
- 普通权限程序无法通过 UIPI 向管理员窗口注入；管理员目标应从已提升的终端单独启动本程序。

## 构建与自动测试

```powershell
cargo test
cargo build --release
target\release\flowtype-windows-inject-spike.exe --self-test
```

## 场景

先在不会产生副作用的空白编辑器中测试：

```powershell
target\release\flowtype-windows-inject-spike.exe --scenario unicode
target\release\flowtype-windows-inject-spike.exe --scenario rewrite
target\release\flowtype-windows-inject-spike.exe --scenario multiline
```

- `unicode`：中文、英文、中文标点、emoji 和组合字符。
- `rewrite`：连续完整状态及任意尾部删除、替换。
- `multiline`：把换行作为 Enter 注入。不要在终端或任何会执行 Enter 的窗口中运行。

只查看 diff 计划而不发送按键：

```powershell
target\release\flowtype-windows-inject-spike.exe --scenario rewrite --dry-run
```

程序不会自动删除测试文字。每个目标应用应使用新的空白文档或输入区。

`--self-test` 会创建一个临时的标准 Win32 多行编辑控件，通过真实 `SendInput` 依次执行三个场景，再读取控件正文进行精确比对。它用于验证操作系统注入基线，不能替代 VS Code、Codex、浏览器和终端的兼容性矩阵。
