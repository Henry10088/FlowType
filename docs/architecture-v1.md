# 说写（FlowType）V1 技术方案

| 项目 | 内容 |
| --- | --- |
| 文档版本 | 0.5 |
| 状态 | 已确认 |
| 日期 | 2026-08-24 |
| 关联需求 | [V1 产品需求基线](requirements-v1.md) |

## 1. 方案目标

技术方案服务于一条明确主路径：Android 当前系统输入法产生文本状态，经过局域网加密连接，到达 Windows 高权限注入助手，并在锁定目标窗口中重现同一文本。

方案优先保证：

1. Android 当前系统输入法产生的新增、删除和替换可以被观察。
2. 中文 Unicode 和任意文本替换正确。
3. 输入过程中没有临时 UAC、配对或恢复弹窗。
4. 网络进程不以管理员权限运行。
5. 依赖和发布体积保持克制。

## 2. 系统拓扑

```text
┌──────────────────────────────────┐
│ Android                          │
│ Kotlin + XML Views               │
│ EditText / Overlay / Keystore    │
│ SQLite / OkHttp WSS client       │
└────────────────┬─────────────────┘
                 │ WSS / TLS 1.3
                 │ JSON full-state protocol
                 ▼
┌──────────────────────────────────┐
│ flowtype.exe                     │
│ 普通权限 Rust 进程                │
│ WSS / 配对 / 认证 / 托盘 / Win32 UI │
└────────────────┬─────────────────┘
                 │ 受限本地命名管道
                 │ 有界的类型化消息
                 ▼
┌──────────────────────────────────┐
│ flowtype-injector.exe            │
│ 当前用户高权限 Rust 进程           │
│ 目标锁定 / Speech TIP 生命周期 / 路由 │
└──────────┬─────────────┬─────────┘
           │             │ Unicode SendInput
           │ TIP 管道    └──────────────────────┐
           ▼                                    ▼
┌──────────────────────────────────┐
│ flowtype_tip.dll                 │
│ 目标进程内 TSF Speech Text Service │
│ 组合范围 / 全文替换 / 本地编辑监听    │
└──────────────────────────────────┘
                                      目标窗口兼容输入后端
```

Android 是当前文本的来源。注入助手持有 Windows 当前会话、目标窗口、输入租约和最后已应用序号；支持 TSF 时，目标 TIP 额外持有组合范围。普通 Windows 主程序只在当前后端确认状态已应用后向 Android 返回 ACK。

## 3. 支持平台

### 3.1 Android

- 最低 Android 10（API 29）。
- `targetSdk` 使用开发时最新稳定版本。
- 不依赖 Google Play 服务，保证中国大陆常见 Android 设备可以离线使用。
- V1 只发布手机界面，不单独适配平板和折叠屏布局。

选择 API 29 的主要原因是直接使用系统 TLS 1.3，避免为旧系统打包额外加密运行时。

### 3.2 Windows

- Windows 10 22H2 x64。
- Windows 11 x64。
- UAC 安全桌面、登录桌面和非当前交互用户会话不支持。
- ARM64 不是 V1 发布目标。

## 4. Android 设计

### 4.1 UI

- Kotlin。
- 单应用模块。
- XML 布局和 Android 原生 Views。
- 不使用 Jetpack Compose、WebView 或跨平台 UI 框架。
- 使用普通多行 `EditText` 接收当前系统输入法。
- 页面采用少量 Activity 或轻量页面容器，避免为三个主页面引入复杂导航框架。
- 完整输入页和悬浮输入面板使用同一个进程级会话控制器；任一时刻只允许一个可编辑视图持有焦点。

### 4.2 输入观察

输入页通过 `TextWatcher.afterTextChanged` 观察 `Editable` 的完整结果。每次变化执行：

1. 读取完整 Unicode 文本。
2. 递增当前会话 `sequence`。
3. 更新内存中的唯一最新状态。
4. 安排一次合并后的加密草稿保存。
5. 将完整状态交给当前 WSS 连接。

网络正常时每次变化立即发送。Android 最多保留一个等待 ACK 的状态和一个最新待发送状态；后者可以被更新状态替换，避免网络不稳时无限积压。草稿落盘允许在不超过 200 ms 的短窗口内合并，并在应用进入后台、切换输入表面和点击完成时立即提交。

### 4.3 网络

- OkHttp WebSocket 客户端。
- 自定义严格的 SPKI 固定校验，以接受说写生成的自签名证书，同时拒绝其他证书。
- 只维持当前选中电脑的业务连接。
- Android 系统 `NsdManager` 发现已绑定电脑。
- 重连采用短间隔指数退避，上限 10 秒；应用回到前台时立即尝试一次。
- 普通模式不使用 Android 前台服务；只有用户开启悬浮输入时运行轻量前台服务。

### 4.4 二维码

V1 使用成熟的 ZXing Android 扫描组件，避免自行承担 Camera2 兼容性。构建后单独记录其 APK 增量；只有测量结果超出体积预算时才评估更小实现。

### 4.5 存储

- 使用系统 SQLite，不使用 Room 和 SQLCipher。
- 历史正文和草稿字段使用 AES-GCM 加密。
- 每台电脑使用独立 Keystore P-256 签名密钥。
- 绑定元数据、历史元数据和设置使用明确版本的数据库 schema。
- 数据库升级必须前向迁移，不在升级时静默清空用户历史。

### 4.6 悬浮输入

- 使用 `TYPE_APPLICATION_OVERLAY`，不使用无障碍服务，也不向手机上的其他应用注入文字。
- 用户从说写设置主动开启功能后，通过 `ACTION_MANAGE_OVERLAY_PERMISSION` 进入系统授权页；授权失败只保持功能关闭。
- 收起窗口使用 `FLAG_NOT_FOCUSABLE`，只有悬浮球命中区域接收触摸，避免影响下层应用。
- 展开快速输入面板时临时取得焦点，显示原生多行 `EditText` 并通过 `InputMethodManager` 唤起当前系统输入法；收起后恢复非焦点窗口。
- 悬浮服务只负责窗口生命周期、手势和前台通知，文本变化仍进入与完整输入页相同的会话控制器、加密草稿和 WSS 队列。
- 从完整输入页与悬浮面板切换时，先解绑旧视图的监听，再恢复文本并绑定新视图，避免程序化赋值生成重复 `sequence`。
- 拖动使用系统触摸阈值区分单击、双击和拖动；释放后按安全区域吸附左右边缘，横竖屏位置分别保存。
- 双击通过普通 Activity 启动路径打开完整应用；当前草稿和会话不复制、不重建。
- 功能开启期间使用低优先级前台服务通知。开启状态持久化，设备重启后只走系统允许的标准恢复路径，不增加厂商专用保活逻辑。

## 5. Windows 主程序

### 5.1 进程职责

`flowtype.exe` 以普通权限运行，负责：

- WSS 服务端。
- TLS 证书和电脑身份。
- 手机配对、认证和解绑。
- 单活跃会话仲裁。
- 局域网发现广播。
- Win32 托盘和管理窗口。
- 将验证后的完整文本状态转交注入助手。
- 在助手确认后返回 ACK。

主程序不操作目标文本，也不把任意网络消息直接转发给注入助手。

### 5.2 UI

- 使用 `windows` crate 调用 Win32 和 Common Controls v6。
- 启用 Per-Monitor DPI Awareness V2。
- 使用 Segoe UI、系统主题、系统键盘导航和无障碍语义。
- 使用 Win32 ListView、Button、Static、Tab 和 Menu 等标准控件。
- 二维码生成后渲染为位图，显示在静态区域。
- 不实现自定义动画、浏览器布局引擎或整套自绘控件。

### 5.3 线程模型

- 主线程运行 Win32 消息循环。
- 独立网络线程运行最小特性集 Tokio runtime。
- UI 与网络通过有界 channel 通信。
- 网络线程不得直接操作 HWND；UI 更新通过窗口消息投递回主线程。

### 5.4 主要依赖

- `windows`：Win32、DPAPI、进程、命名管道和 UI。
- `tokio`：异步网络运行时，只启用实际使用的特性。
- `tokio-tungstenite`：WebSocket 服务端。
- `rustls`：TLS 1.3。
- `serde` / `serde_json`：业务协议。
- `windows` 的 TSF 接口：目标组合范围和 Unicode 全文替换。
- `windows-sys`：Unicode `SendInput` 兼容后端和真实键鼠输入监测。
- `qrcode`：绑定二维码。
- 证书生成使用经过审查、与 `rustls` 兼容的最小依赖实现。

## 6. 高权限注入助手

### 6.1 生命周期

- 首次安装时创建当前用户登录触发的高权限计划任务。
- 注入助手在交互用户桌面中运行，不作为 Windows Service 运行。
- 登录后自动启动，不在用户开始输入时再申请 UAC。
- 助手没有托盘、窗口、网络监听和自动更新逻辑。
- 安装程序注册 `flowtype_tip.dll`；卸载前先停助手，再注销 DLL。
- 安装时启用 FlowType Speech Profile；注入助手在登录后确保它只激活一次，并在整个桌面会话中保持空闲待命。
- 普通输入完成、取消、断线、错误和助手重启都不得停用或禁用 Profile；只有卸载或用户注销结束其系统生命周期。
- FlowType 不注册 `GUID_TFCAT_TIP_KEYBOARD`，不出现在 `Win + Space` 键盘列表，也不切换或恢复用户键盘输入法。

### 6.2 本地管道安全

- 使用命名管道，DACL 只允许当前用户、Administrators 和 SYSTEM。
- 注入助手获取连接方 PID 并校验主程序安装路径。
- 发布版本校验主程序 Authenticode 签名或安装时固定的文件身份。
- 主程序安装目录位于普通用户不可修改的 `Program Files`。
- IPC 使用有最大长度限制的类型化消息，不接受命令行、脚本、路径或任意虚拟键。
- 单条 IPC 消息上限与 WSS 文本消息上限保持一致。

### 6.3 接口范围

助手只接受：

```text
begin_session(session_id, sequence)
apply_state(session_id, sequence, full_text)
finish_session(session_id, sequence)
query_status()
cancel_invalid_session(session_id)
```

`cancel_invalid_session` 只清理助手内存状态，不向目标窗口发送删除或恢复操作。

## 7. 输入后端与会话租约

### 7.1 Speech Text Service

- `flowtype_tip.dll` 是进程内 COM Text Service，注册类别为 `GUID_TFCAT_TIP_SPEECH`。
- Speech 类别与用户当前键盘 Profile 并行；说写不得注册为键盘 TIP，也不得调用键盘 Profile 激活接口。
- 注入助手启动时一次性激活 FlowType Speech Profile，TSF 将 DLL 加载到当前及后续支持 TSF 的目标进程；每段手机输入不再触发 Profile 变更。
- 每个 TIP 实例通过受限命名管道上报 PID 和线程 ID；助手只向锁定目标的实例发送有界类型化命令。
- V1 同时发布 x64 和 x86 DLL。

### 7.2 双后端状态应用

1. `begin_session` 捕获当前前台 HWND、PID 和 GUI 线程 ID。
2. 注入助手优先在目标 TIP 的焦点 `ITfContext` 调用 `StartComposition`；短时间内无法建立时使用 Unicode `SendInput`，不维护应用名称白名单。
3. TSF 后端通过 `ITfRange::SetText` 替换组合范围；兼容后端只对已确认的末尾变化发送退格和新增文本，中间变化才保守重写本会话文本。
4. 两个后端都承载中文、换行、emoji 和非 BMP 字符，不切换用户当前键盘输入法。
5. `finish_session` 提交 TSF 组合或释放兼容后端状态，FlowType Speech Profile 保持激活和空闲。
6. TSF ACK 表示范围编辑已成功；兼容后端 ACK 表示全部 `SendInput` 事件已被系统接受，无法证明目标应用已经持久化文本。

同一序号与同一全文是幂等重复；同一序号对应不同全文立即终止会话。生产路径不使用剪贴板粘贴、UI Automation 或应用专用插件。

### 7.3 输入权与目标锁定

- 每次更新前要求原 HWND 仍有效且仍是前台窗口；TSF 后端还要求焦点 `ITfContext`、组合文本和选区仍与上次 ACK 一致。
- 注入助手使用低级键鼠事件监测建立会话输入租约；真实按键、点击或滚轮使租约失效，FlowType 自己注入的事件和单纯移动鼠标不影响租约。
- TIP 通过 `ITfTextEditSink` 观察组合上下文，并用内部标记区分自己的远程编辑与外部编辑。
- 电脑端打字、点击新光标、移动选区、其他输入法结束组合或应用主动修改文本时，助手结束旧后端并返回 `target_modified`。
- `target_modified` 后 Windows 不再应用手机更新；Android 保留完整正文并明确显示“电脑端已编辑，本次同步已停止”。
- 目标失焦时不修改文本；目标恢复前台后只有在原组合仍有效时才能继续。
- HWND 关闭、进程退出、TIP 卸载或组合被目标终止时，不猜测恢复、不发送删除操作。
- 自动验收记录一次性激活前后的键盘 Profile 和目标线程 HKL；连续多轮开始、完成、取消和失败后这些状态必须完全一致。
- TIP 管道空闲时使用阻塞读取，不以定时轮询维持常驻服务。

## 8. WSS 与身份认证

### 8.1 电脑身份

- 首次安装生成长期 P-256 电脑身份私钥。
- 私钥通过当前用户 DPAPI 加密后保存。
- WSS 使用该身份生成自签名 TLS 证书。
- Android 固定证书公钥 SPKI 哈希，而不是依赖系统 CA。
- 证书更新时只要电脑身份公钥不变，绑定关系可以保持。

### 8.2 配对二维码

二维码包含版本化数据：

```text
protocol_version
pc_id
candidate_endpoint
candidate_endpoints
tls_spki_sha256
one_time_pairing_token
```

`candidate_endpoint` 是兼容旧版手机的首选地址。`candidate_endpoints` 包含同一台电脑的
Tailscale 和物理局域网地址，过滤 Hyper-V、WSL 等已知虚拟交换网卡。Windows 同时
监听所有网卡；Android 每次只建立一个连接，连接失败时依次尝试候选地址，并将最近
连接成功的地址保存为下次首选。
Windows 还会在连接成功的 `ready` 消息中返回最新地址候选，使升级前已绑定的手机无需
重新扫码即可补全地址。

配对令牌使用密码学安全随机数，只保存在 Windows 内存中。二维码窗口关闭、绑定成功或重新生成二维码后立即失效。

### 8.3 手机身份

配对时 Android 为该电脑生成 P-256 签名密钥，将公钥通过已固定 SPKI 的 WSS 连接注册到 Windows。后续连接流程为：

1. Windows 发送随机 nonce。
2. Android 对带协议域分隔符、电脑 ID、手机 ID 和 nonce 的内容签名。
3. Windows 使用绑定公钥验证签名。
4. 验证成功后进入业务协议。

nonce 每次连接重新生成，防止重放。

## 9. 业务协议

### 9.1 编码

- WebSocket 文本帧。
- UTF-8 JSON。
- 所有消息包含 `protocol_version`、`type` 和消息相关字段。
- 单条文本消息默认上限 1 MiB，超过后返回明确错误，不分配无限内存。
- 未知必需字段、错误类型和超限内容必须拒绝。
- 心跳使用 WebSocket 原生 Ping/Pong，不定义 JSON 心跳消息。
- V1 不启用 WebSocket 消息压缩，也不对 TLS 连接内的每条业务消息重复签名。

文本流量不值得为 CBOR 增加跨语言调试成本。图片使用 `image_start` JSON 元数据帧和紧随其后的独立二进制帧，复用现有认证后的 WSS 连接。

### 9.3 文件传输

文件传输不复用文字 WebSocket 的正文通道，避免大文件排队影响实时输入。控制面继续使用已认证的 WSS，负责批次清单、能力协商、接受/拒绝、进度、取消和断点状态；正文使用独立 HTTPS/TLS 文件通道流式传输。Windows 作为文件服务端，手机上传使用 `POST`，电脑发送时手机通过出站 `GET` 下载，因此 Android 不需要开放入站端口。

文件批次支持多文件、目录和 Windows 资源管理器拖放。清单使用 64 位大小、相对路径、文件类型、修改时间和 SHA-256，不设置软件层面的大小上限。接收端将正文写入临时文件并持久化已确认偏移；重连后跳过已完成文件，从当前文件偏移继续，完成哈希校验后再落盘。符号链接、Junction 和 Reparse Point 默认不跟随，文件冲突自动改名而不覆盖。

手机主动发送的批次由 Windows 自动保存到配置目录；电脑发送到手机时由 Android 对整个批次确认一次并选择目标目录。Android 在后台使用前台服务保持传输并显示通知。详细产品边界见 [文件传输设计基线](file-transfer-design-v1.md)。

局域网不视为可信边界。TLS 1.3、证书 SPKI 固定、绑定身份认证和每批次一次性令牌始终启用；“安全传输”只是默认开启的增强模式，不允许切换为明文或绕过绑定认证。

### 9.2 会话状态

```text
idle
  -> starting
  -> active
       -> waiting_target -> active
       -> reconnecting   -> active / waiting_target
       -> finishing      -> finished -> idle
       -> target_invalid
       -> target_modified
       -> injection_unknown
```

用户界面不直接展示这些内部名称，而是映射为具体原因和操作提示。

- 正常连接时，第一次非空变化创建 `session_id`，`start` 同时携带第一份完整文本和 `sequence`。
- `waiting_target` 和 `reconnecting` 期间 Android 继续编辑并保存最新完整状态。
- `finishing` 期间 Android 立即禁止编辑，收到最终 ACK 后才写入历史并创建新的空闲输入。
- 用户选择“放弃同步并结束”时，Android 写入本地历史、清空待发送状态并发送 `cancel`；Windows 仅释放助手会话，不再修改目标文本。
- 活跃会话删除到空文本后仍可完成；最终空状态应用成功后关闭会话，但不创建空历史。
- 电脑离线时产生的新草稿没有可验证目标，只保存在 Android。本次连接恢复后必须由用户放好光标并执行“同步全文”，不能自动锁定碰巧处于前台的窗口。
- Windows 主程序、注入助手或目标进程失效后进入 `target_invalid`，只允许用户从新光标创建新会话。
- 电脑端主动编辑或移动选区后进入 `target_modified`；Windows 立即让出输入权，Android 保留正文等待用户重新同步。
- TSF 编辑结果无法确认时进入 `injection_unknown`。助手不更新逻辑文本、不自动重试。

### 9.3 顺序

- `sequence` 使用从 1 开始的正数 64 位有符号整数，不要求连续。
- Android 每次 `Editable` 变化递增一次。
- `start`、`update` 和 `finish` 都携带对应序号的完整文本状态。
- Windows 只向注入助手提交比最后已应用序号更新的状态。
- 助手尚未开始处理的多个更新可以合并到最新状态；TIP 已确认的状态不能假装未发生。
- ACK 表示目标 TIP 已通过 TSF 将完整状态应用到组合范围，不表示目标应用已经持久化文本。
- ACK 为累计确认；确认序号 N 表示目标中的会话文本与状态 N 一致，低于 N 的中间状态不再需要处理。
- 同一 `session_id` 和 `sequence` 出现不同 `full_text` 时终止该会话并返回协议错误。
- 完成 ACK 后服务端缓存该会话的终态标识，连接存续期间拒绝后续修改。

### 9.4 存活与重连

- WSS 定期使用协议原生 Ping/Pong 检测失活连接。
- Android 重连退避上限 10 秒。
- 应用进入前台、网络重新可用和局域网发现地址变化时立即触发重连。
- 重连成功后先交换最后 ACK，再恢复业务状态。
- 同一电脑与手机只保留一条已认证连接；新连接认证成功后，旧连接不再允许提交业务消息。

V1 不持久化 Windows 输入事务日志，也不承诺跨 Windows 主程序、注入助手或电脑重启自动实现恰好一次恢复。此类情况下保留 Android 全文，由用户在新光标处重新输入。

## 10. 局域网发现

- Windows 使用系统 DNS-SD API 注册说写服务。
- Android 使用 `NsdManager` 浏览服务。
- 服务记录只包含随机电脑 ID、端口和协议版本。
- 手机只尝试认证已绑定的电脑 ID。
- 绑定二维码提供初始地址；发现失败时尝试上次成功地址。

### 10.1 多电脑自动选择

- Android 只维持一个长期业务 WSS 连接；自动选择使用每台候选电脑一个短生命周期的认证探测连接。
- 探测消息只包含手机 ID，不携带当前正文、会话 ID 或草稿；收到结果后连接立即关闭。
- Windows 探测当前前台窗口、目标进程内 FlowType TIP 是否可用和本机 `GetLastInputInfo` 活动年龄。
- 手机在新输入的第一个 `start` 之前完成选择；探测期间继续更新本地完整正文，但不向任何候选电脑发送正文。
- 选择结果按以下优先级确定：唯一可用候选；当前电脑与最佳候选活动年龄差不超过 2 秒；最佳候选活动年龄不超过 3 秒；或最佳候选领先第二候选至少 2 秒。
- 其余情况视为无法确认，保留手机正文并要求手工选择。选定后会话锁定该电脑，后续修正、删除、替换和 `finish` 均不得重新选择。
- 探测连接有 2.5 秒硬超时；超时、认证失败或网络不可用只淘汰该候选，不阻塞其他候选结果。

使用系统 API 可以避免额外 mDNS 运行时，并减少不同实现之间的网络行为差异。

## 11. 本地数据

### 11.1 Android

SQLite 数据至少分为：

- 电脑绑定。
- 当前会话。
- 已完成历史。
- 设置。

正文、草稿和敏感连接字段加密后入库。密钥别名和数据库 schema 均带版本号。

### 11.2 Windows

Windows 只持久化：

- DPAPI 加密的电脑私钥。
- 已绑定手机公钥和名称。
- 电脑名称、开机启动等设置。

当前注入正文只存在于注入助手内存中。助手退出、电脑重启或目标失效后不尝试从磁盘恢复正文。

## 12. 安装与升级

V1 使用 Inno Setup：

1. 申请一次安装级 UAC。
2. 安装两个 Rust 可执行文件和一个 TSF DLL 到 `Program Files`，通过 `regsvr32` 注册 Speech Text Service。
3. 为普通主程序创建当前用户登录启动项。
4. 为高权限注入助手创建最高权限计划任务。
5. 创建远端范围为 `LocalSubnet` 的入站防火墙规则。
6. 启动主程序并打开首次配对窗口。

V1 不实现自动更新。新版安装程序执行覆盖升级，保留当前用户绑定数据。卸载时询问是否删除用户数据；计划任务、防火墙规则和启动项必须移除。

正式发布必须评估 Authenticode 代码签名。未签名安装包导致的 SmartScreen 警告是产品易用性问题，不作为普通用户发布形态。

## 13. 体积控制

- Android 不使用 Compose、Room、SQLCipher、Google Play Services 和遥测 SDK。
- Windows 不使用 Slint、WebView2、WinUI 3、Skia 或 wgpu。
- Rust crate 关闭默认功能，只启用实际需要的后端和算法。
- Release 使用 LTO、单 codegen unit、`panic=abort` 和符号剥离。
- 安装包使用压缩后的 Release 产物，不附带调试符号。
- 文件传输使用 Android 系统文件选择器、现有 OkHttp/TLS 和 Rust 现有 TLS 基础，不引入媒体库、浏览器内核或额外加密运行时。

文件传输的初步增量估算：Android APK 约 20-100 KiB，Windows 主程序和压缩安装包各约 150-500 KiB，注入助手和 TIP 不增加。无限文件大小不会增加安装包体积，只增加运行时磁盘和恢复状态需求；实现后必须以干净 Release 构建复测。

首个完整 Release 构建的目标：

| 产物 | 初始目标 |
| --- | ---: |
| Android APK | 不超过 10 MiB |
| Windows 主程序 | 不超过 8 MiB |
| Windows 注入助手 | 不超过 1 MiB |
| Windows TSF DLL | 不超过 1 MiB |
| Windows 安装包 | 不超过 12 MiB |

这些是体积门槛，不是通过删除核心安全和同步功能强行达到的指标。超出后先分析依赖构成，再决定优化。

## 14. 实现验收门槛

正式实现按阶段持续验证，发布前必须满足：

1. Android 当前系统输入法的新增、删除和替换会触发可观察的 `Editable` 变化。
2. TSF 组合范围可以原位应用中文、任意替换、删除、emoji 和换行。
3. FlowType Speech Profile 激活前、中、后，用户键盘 Profile 保持完全一致。
4. 电脑端主动编辑时 FlowType 立即让出，不覆盖本地输入，Android 全文仍可恢复。
5. 高权限计划任务中的注入助手和目标进程内 TIP 可以覆盖普通及管理员目标。
6. WSS、完整状态消息和 ACK 在普通家庭局域网中达到 P95 100 ms 目标。
7. 精简 Win32、TLS 和二维码依赖后的 Release 体积满足初始门槛。

任一核心门槛失败时，先记录真实证据并修订技术方案，不继续堆叠上层 UI 或恢复逻辑。
