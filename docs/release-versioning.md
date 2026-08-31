# 发布版本规范

## 应用版本

说写使用 SemVer：`MAJOR.MINOR.PATCH`。

- `MAJOR`：不兼容的产品或数据迁移变更。
- `MINOR`：向后兼容的新功能。
- `PATCH`：向后兼容的问题修复、稳定性和兼容性改进。

在 `1.0.0` 之前，项目处于内测阶段：新增功能提升 `MINOR`，修复问题提升 `PATCH`。达到 V1 发布条件后再进入 `1.x`。

任何可安装、可分发或提供给用户升级的产物都必须递增版本号；同一个平台的版本号禁止对应不同内容。Windows 和 Android 使用独立版本号、独立 Git 标签和独立 GitHub Release：标签分别为 `windows-vMAJOR.MINOR.PATCH`、`android-vMAJOR.MINOR.PATCH`。

Android 的 `versionCode` 必须在每次 Android 发布时严格递增。Windows 修复只发布 Windows，不要求重新构建 Android；Android 更新同理。两端仍共享协议版本和产品主版本策略，但不再强制版本号相同。

当前版本：Windows `0.2.8`；Android `0.2.4`，`versionCode=24`。

当前 Android-Windows 网络协议为 v2。v2 的 `start` 可携带待替换的旧会话 ID，使“电脑端已编辑后同步到新光标”在取消消息丢失、断线和 Android 进程恢复后仍可原子重试。该变更不兼容 v1，两端必须同时更新。

## 协议版本

网络协议版本独立于应用版本。只有改变协议兼容性时才提升协议版本；普通 UI、输入法、图片和稳定性改动不修改协议版本。

Windows 的两层本地 IPC 也分别维护独立版本：主程序与 Injector 使用 `INJECTOR_IPC_VERSION`，Injector 与进程内 TIP 使用 `TIP_IPC_VERSION`。消息字段、状态机语义、连接生命周期或失败恢复规则发生不兼容变化时，必须提升对应 IPC 版本，不能用应用版本或“消息仍能反序列化”代替兼容性判断。

TIP DLL 被文本宿主加载后通常要等宿主退出才能卸载。Injector 必须拒绝不同 `TIP_IPC_VERSION` 的驻留组件，并保持隔离连接以阻止旧组件频繁重连；不得向不兼容组件发送 `Begin` 或全文快照。发布前需要同时验证旧宿主仍驻留和新宿主首次启动两种升级场景，并用真实文本宿主覆盖换行导致组合态结束后继续全文替换的场景。

## 本地提交与远程发布

- 本地开发阶段允许多次提交、反复构建和验证；本地提交不会自动推送到远程仓库。
- 未收到项目所有者明确的“push 到远程”或等价指令时，不执行 `git push`，不创建或推送远程标签，不触发远程构建和 Release。
- 多次本地提交可以在一次明确授权的 push 中统一同步；一次 push 作为一个远程构建节点，不要求为了打包强行压缩本地提交历史。只有明确要求整理提交历史时才执行 squash 或 rebase。
- 远程版本号只在明确准备远程发布时更新。普通本地修复、临时验证和测试构建不得为了触发打包而修改正式版本号。
- 远程正式发布必须同时满足：版本号已递增、版本检查通过、用户明确授权 push，并按本规范创建对应的 `vX.Y.Z` 标签。测试标签只能用于内部验证，不得替代正式版本。
- 远程构建完成后保留本地工作区和远程 Release 的对应关系；除非用户明确要求，不删除测试标签、Release 或构建资产。

## Git 配置保护

- Git 配置属于项目所有者的本机环境，不是提交、构建或发布流程的一部分。
- 除非项目所有者在当前指令中明确要求修改具体配置项，否则禁止执行任何作用域的 `git config` 写操作，包括 `user.name` 和 `user.email`。
- 早期对话、旧提交、终端历史和其他仓库中的配置只能作为历史信息，不能作为修改当前 Git 配置的依据。
- 提交前可以只读检查 Git 配置。配置缺失或与预期不一致时必须停止提交并向项目所有者确认，禁止自动补写、回退到旧值或使用命令行临时覆盖。
- 个人本机 Git 名称、邮箱或其他私有配置不得写入仓库文档作为项目固定值。
- 修改已有提交的 author、committer 或标签 tagger 属于历史重写，必须得到项目所有者明确授权；涉及远程时还必须单独满足本节的远程 push 授权。

## 发布检查

1. 只修改目标平台的版本文件：Windows 修改 `windows/Cargo.toml` 和 `installer/flowtype.iss`；Android 修改 `android/app/build.gradle.kts` 及显示版本资源。
2. 运行 `scripts\verify-version.ps1 -Platform Windows` 或 `-Platform Android`，确认目标平台版本高于其最近一次发布标签。
3. 只运行目标平台测试、签名构建和产物校验。
4. 使用目标平台标签 `windows-vX.Y.Z` 或 `android-vX.Y.Z` 触发对应 GitHub Release。
5. Windows TIP 或 Injector 行为有变化时，运行同一连接的 `Begin -> Update* -> Finish` 回归和真实文本宿主测试；若兼容语义变化，先提升对应本地 IPC 版本，再验证旧驻留 DLL 被隔离且不会重连刷日志。
