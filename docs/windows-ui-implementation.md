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
