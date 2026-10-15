# Windows 原生界面

视觉基准：`prototypes/windows-ui`。正式程序继续使用 Win32 子控件、Direct2D/DirectWrite 绘制和 GDI 回退；原型中的演示设备、文件和模拟扫码按钮不进入正式程序。

## 页面与真实操作

- 状态：实际连接、输入目标与地址；绑定按钮打开真实配对二维码。
- 已绑定手机：真实绑定记录和连接状态；解除绑定沿用原有确认流程。
- 配对：实际配对 URI 的二维码；绑定关系不设到期时间，二维码本身只用于一次配对。
- 文件传输：发送和收到标签只展示当前批次中符合各自方向的进度、失败文件和结果；两个方向都可直接取消。另一方向正在传输时显示前往对应标签查看的提示。取消绑定到页面上显示的批次 ID，避免完成与新批次交替时取消错误任务。收到页保留实际接收目录和更改位置操作。没有持久的成功文件列表时不展示原型中的示例文件。
- 设置：电脑名称、开机启动、悬浮球、输入服务和更新仍使用现有操作。

## 预览与验证

`flowtype.exe --ui-preview --show` 不启动网络监听，可与已安装程序并行。设置 `FLOWTYPE_UI_PREVIEW_PAGE` 为 `status`、`files`、`phones`、`pairing` 或 `settings` 可直接查看指定页面。`FLOWTYPE_UI_FORCE_GDI=1` 强制 GDI；debug 构建用 `FLOWTYPE_UI_SIMULATE_PAINT_FAILURE=1` 模拟一次 Direct2D 绘制失败。

已在开发机目视检查状态、文件发送/收到、手机及配对页面。设置页和模拟绘制失败后的自动截图因桌面捕获返回黑画面/访问拒绝，尚待交互式视觉复核；日志确认失败进入 GDI 回退，没有窗口退出。还需在 Win10 IoT、不同 DPI、RDP 重连环境复核。安装包、空闲工作集增量须以相同环境重测，不能仅凭 debug 与后台进程对比。

## 2026-10-14 审查修复

- 图标掩码长度使用整数 `div_ceil`，满足 CI 的 Clippy 警告检查。
- 两个文件标签共用初次创建和增量更新逻辑，接收页补齐取消操作；对应回归覆盖方向隔离、终态、断线和新批次替换。
- “输入状态”绑定已有运行状态，随输入、目标失效和服务错误刷新，不再固定显示等待输入。
- 配对页返回按钮使用独立控件 ID，窗口缩放时按新的右边界重新定位，不影响侧栏的手机导航。
- 最小客户区统一为 780 × 600 DIP。更新状态与进度条使用共享布局，缩放时同步调整宽度；布局检查覆盖 100%、125%、150% 和 200% 的坐标换算，验证底部留白及接收取消按钮不与说明重叠。
- 构建和测试使用独立目录 `windows/target/ui-review-fixes`，避免其他工作副本共用 `target` 导致测试复用错误构建。布局单元测试不能替代真实多显示器和 RDP 验收。

本次验证结果：

- `cargo fmt --all -- --check` 通过。
- `cargo test --workspace --locked --target-dir target/ui-review-fixes`：120 项通过、6 项按原有条件忽略，包含新增的 5 项 UI 状态与布局回归测试。
- `cargo clippy --workspace --all-targets --locked --target-dir target/ui-review-fixes -- -D warnings` 通过。
- `cargo build -p flowtype-app --bin flowtype --release --locked --target-dir target/ui-review-fixes` 通过。主程序为 2,687,488 字节（约 2.56 MiB），仅用于本地验证，不是安装包；本次未替换已安装程序，也未生成正式发布产物。
- 本次尚未复验真实窗口视觉、跨 DPI、RDP 重连和 Win10 IoT；未测量空闲工作集，不能据此宣称安装包与内存增量目标已达成。

## 2026-10-15 合并主干

- 合并文件传输和原生 UI 分支，保留主干的 TSF 宿主退出卡死修复、悬浮球重绘修复、MDL2 图标兼容，以及移除 Windows mDNS / Android NSD 的改动。
- Windows 升级为 `0.3.1`；Android 保留分支已有的 `0.3.0`，本次不构建或安装 Android 发布包。
- 合并结果通过 Windows workspace 123 项测试，6 项既有压力/交互测试按条件忽略；格式检查、Clippy `-D warnings`、x64 workspace Release 和 x86 TIP Release 构建通过。使用独立目录 `windows/target/main-merge-install`。
- 合并后的 Android 78 项 Debug JVM 测试通过。本轮未执行真机联调或仪器测试。
- 本机安装包为明确标记的本地未签名构建，不标记为正式公开发布。签名步骤被工具的自动审批策略拒绝，返回原因为 `blocked by policy`，因此没有使用本地签名凭据；真实跨 DPI、RDP 和 Win10 IoT 验收仍待完成。

### 本机覆盖升级结果

- 安装包：`installer/output/FlowType-0.3.1-x64-local-unsigned-setup.exe`，3,367,901 字节（约 3.21 MiB）；SHA-256：`96632762dc689ef7de497954fbff2647ac76db641fa277a365af2088ae35ef89`。未创建公开 Release 或发布标签。
- 覆盖升级前备份现有程序及用户数据到被 Git 忽略的 `dist-local/`。安装程序退出码为 0，不要求系统重启；卸载信息和主程序文件版本均为 `0.3.1`。
- 主程序和 Injector 的安装文件哈希与本次构建一致；x64、x86 COM 注册分别指向本次修复后的 TIP DLL，两个已注册文件的哈希与构建一致。
- 主程序已从安装目录启动，窗口响应正常，WSS 32187 和文件 HTTPS 32188 端口均在监听；Injector 计划任务运行中。这些检查不能替代手机输入或文件传输的现场验收。
- 身份、绑定手机、悬浮球显示与位置、语言文件均与升级前逐一比对哈希一致。安装器会自动打开开机启动，本次已恢复该电脑原有的关闭状态；未卸载或清空用户数据。
