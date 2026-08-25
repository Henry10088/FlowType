# 发布版本规范

## 应用版本

说写使用 SemVer：`MAJOR.MINOR.PATCH`。

- `MAJOR`：不兼容的产品或数据迁移变更。
- `MINOR`：向后兼容的新功能。
- `PATCH`：向后兼容的问题修复、稳定性和兼容性改进。

在 `1.0.0` 之前，项目处于内测阶段：新增功能提升 `MINOR`，修复问题提升 `PATCH`。达到 V1 发布条件后再进入 `1.x`。

任何可安装、可分发或提供给用户升级的产物都必须递增版本号；即使只是 Windows 修复，也要提升 `PATCH`。同一个版本号禁止对应不同内容，不能用新构建覆盖旧安装包。仅本地开发或临时验证、且不交付给用户的构建可以沿用当前开发版本，但不得作为发布安装包。

Android 的 `versionCode` 必须在每次发布时严格递增，即使 `versionName` 只增加补丁号。Windows、Android、安装包文件名和 Git 标签使用同一个 `versionName`，Git 标签格式为 `vMAJOR.MINOR.PATCH`。

当前版本：`0.1.14`，Android `versionCode=15`。

本次从旧工程标识迁移到 FlowType 会更换 Android applicationId、绑定 URI、认证前缀、mDNS 服务名和本地 IPC 名称；JSON 消息结构未变化，因此协议字段版本仍为 1。该迁移是 V1 内测阶段的兼容性断点，旧版本必须卸载并重新绑定，不承诺跨版本互通。

## 协议版本

网络协议版本独立于应用版本。只有改变协议兼容性时才提升协议版本；普通 UI、输入法、图片和稳定性改动不修改协议版本。

## 发布检查

1. 确定递增后的版本号并同步 `windows/Cargo.toml`、`android/app/build.gradle.kts` 和 `installer/flowtype.iss`；不要修改历史验证文档中的版本证据。
2. 运行 `scripts\verify-version.ps1`，确认版本号没有漂移且高于最近一次发布标签（在发布标签本身上允许相等）。
3. 更新 Android `versionCode`，运行 Android 测试、Lint 和 Release 构建。
4. 运行 Windows workspace 测试、Clippy 和 Release 构建，并编译安装包。
5. 使用签名材料生成两端发布包，记录 SHA-256。
6. 提交代码后创建 `vX.Y.Z` 标签，再上传 GitHub Release。
