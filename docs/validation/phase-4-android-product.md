> 历史记录：本文保留阶段性 Android 产品验证证据，不代表当前版本的完整兼容性承诺。文中的旧文件名可能已变更，当前名称见 [验证记录说明](README.md)。

# 阶段 4 Android 完整产品验证记录

| 项目 | 内容 |
| --- | --- |
| 日期 | 2026-08-23 |
| 设备 | Android Emulator `Learn_API_36`，Android 16 / API 36 |
| 结果 | 阶段 4 实现与模拟器退出检查通过 |

## 实现结果

- 输入、历史、历史详情、电脑和设置使用 Kotlin、XML 与原生 Views，不使用 Compose。
- `FlowTypeApplication` 持有唯一进程级会话、WSS 队列和草稿状态；完整输入页与悬浮面板不建立第二套会话。
- 原生 SQLite 保存多电脑、选择、设置和历史元数据；旧版单电脑偏好自动迁移。
- 历史正文和草稿分别使用 Android Keystore AES-GCM 密钥加密。
- 历史支持倒序列表、只读详情、复制、作为新输入和删除。
- 电脑支持扫码添加、选择、重命名和确认解绑；活跃会话期间阻止切换、扫码和解绑当前电脑。
- `NsdManager` 发现 `_flowtype._tcp.` 服务，只匹配已绑定电脑 ID，并更新当前局域网地址。
- 扫码迁移到 `ScanContract` Activity Result API。
- 输入页默认常亮，极暗模式与设置持久化，离开输入页恢复系统亮度。
- 悬浮输入使用 `TYPE_APPLICATION_OVERLAY` 和 `specialUse` 前台服务；支持单击展开、拖动吸边、双击进入完整应用、底部关闭区和静默通知关闭动作。
- Android 12 及以上数据提取规则排除全部应用数据，敏感状态不参与备份或设备迁移。

## 自动验证

```text
./gradlew testDebugUnitTest lintDebug connectedDebugAndroidTest assembleRelease

JVM tests:       14 passed, 0 failed
Device tests:     3 passed, 0 failed
Lint:             0 errors
Release build:    passed
```

设备测试覆盖：

- 多电脑永久保存和当前电脑选择。
- 历史 AES-GCM 回读，SQLite BLOB 中找不到测试正文。
- 草稿 AES-GCM 回读，SharedPreferences 中找不到测试正文。

## 模拟器可见检查

- Android 16 强制边到边环境下，输入、历史、电脑和设置页面均正确避开状态栏和导航栏。
- 输入页使用纯黑背景、统一字号层级和唯一强调色 `#00BAB8`。
- 历史空态、电脑离线状态、重命名/解绑动作和设置开关可见且无重叠。
- 授予系统悬浮权限后，离开完整应用会显示 56dp 触控区的半透明悬浮球。
- 悬浮球单击展开快速输入面板；输入区取得焦点后系统 Gboard 正常显示，面板位于输入法上方。
- 前台服务保持运行并使用 `specialUse` 类型；返回完整应用时覆盖层隐藏。

## 体积

```text
app-release-unsigned.apk: 842,645 bytes
```

加入完整页面、Activity Result、SQLite 数据层和悬浮服务后仍远低于 10 MiB 预算。

## 验证边界

- 相机扫码在模拟器会进入系统相机权限流程；二维码注册和永久认证已在阶段 2、3 实测。
- 模拟器验证使用普通系统 Gboard。真实手机的厂商悬浮策略、相机和系统输入法矩阵留到阶段 6，不预置厂商专用保活分支。
- 阶段 5 完成 Windows mDNS 发布后，再进行地址变化后的两端联合发现测试。
