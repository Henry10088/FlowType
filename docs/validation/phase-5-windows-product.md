> 历史记录：本文保留阶段性 Windows 产品验证证据，不代表当前版本的完整兼容性承诺。文中的旧文件名可能已变更，当前名称见 [验证记录说明](README.md)。

# 阶段 5 Windows 完整产品验证记录

日期：2026-08-23

## 完成范围

- 原生 Win32 Common Controls v6 管理窗口：状态、已绑定手机、设置和二维码绑定。
- 系统托盘状态、打开、绑定新手机和明确退出。
- Per-Monitor DPI v2、最小窗口尺寸、键盘 Tab 顺序和 Windows 默认亮色。
- 电脑名称、登录自动启动、手机解绑和输入服务修复。
- TLS/WSS 监听与 `_flowtype._tcp.local.` mDNS 发布运行在独立网络线程。
- 网络线程只更新共享状态，通过 `WM_APP` 通知 UI，不跨线程直接操作窗口。
- 单实例互斥；再次启动会唤醒已有窗口。
- Inno Setup 安装两个 Release 程序，集中创建高权限计划任务和 LocalSubnet 防火墙规则。
- 覆盖安装不删除 `%LOCALAPPDATA%\FlowType`，卸载清理计划任务、防火墙和当前用户自启动。
- 安装和卸载时以原登录用户身份设置自启动，避免管理员凭据属于另一账户时写错 `HKCU`。

## 自动验证

```text
cargo test --workspace
11 passed, 0 failed

cargo clippy --workspace --all-targets -- -D warnings
passed

cargo build --workspace --release
passed

Inno Setup 6.7.3 compiler
successful compile, 0 warnings
```

运行态检查：

- `flowtype-app.exe --show` 创建可响应的“说写”窗口。
- `0.0.0.0:32187` 处于监听状态。
- 连续启动两次后进程数保持为 1，已有窗口收到显示请求。
- API 36 模拟器中的 Android 应用已注册 `_flowtype._tcp.local` 发现请求。
- 模拟器虚拟网络未收到宿主机局域网组播，系统记录 `foundServices 0`；不将该结果视为真机 mDNS 验收。

## Release 体积

| 产物 | 大小 |
| --- | ---: |
| `flowtype-app.exe` | 1,742,336 bytes |
| `flowtype-injector.exe` | 210,944 bytes |
| `FlowType-0.1.0-x64-setup.exe` | 2,865,130 bytes |

## 尚未在本阶段声称通过的项目

- 当前 Codex 窗口站无法可靠操作用户桌面，未执行完整 UI 点击矩阵。
- 未在本机执行会改变计划任务、防火墙和 Program Files 的安装、覆盖升级与卸载验收。
- Android 模拟器不能替代同一物理局域网中的真机 mDNS 验收。
- VS Code、Codex、Chrome、Edge、Windows Terminal、PowerShell 及管理员目标的注入矩阵进入阶段 6。
- Windows Authenticode、Android Release 签名和 SmartScreen 流程进入阶段 6。

阶段 5 的产品代码和可安装产物已完成；上述真实环境矩阵作为阶段 6 发布阻断项保留，不以模拟结果替代。
