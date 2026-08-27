# 说写（FlowType）GitHub 在线更新设计基线

| 项目 | 内容 |
| --- | --- |
| 文档版本 | 0.1 |
| 状态 | 已实现，待正式 Release 端到端验收 |
| 日期 | 2026-08-26 |
| 适用范围 | Windows x64、Android |
| 发布源 | [GitHub Releases](https://github.com/Henry10088/FlowType/releases) |

## 1. 目标与边界

在线更新用于让已经安装的 Windows 和 Android 客户端发现、下载并安装 GitHub Releases 中的正式版本。

第一版确认以下原则：

- 不建设后端、对象存储或 CDN，直接使用 GitHub Releases 地址。
- GitHub Actions 根据版本标签自动构建、签名并发布 Windows 安装包和 Android APK。
- 客户端自动检查更新，也提供手工“检查更新”；检查或下载失败不影响实时输入、局域网连接和其他功能。
- 下载在后台执行，显示明确进度，并使用操作系统下载能力实现可恢复下载。
- 下载完成后仍由用户确认安装，不做静默安装、强制更新或自动重启应用。
- 不实现第二套自研下载器。BITS 或 Android 系统下载失败时说明原因，并提供重试和打开发布页。
- 复用 WinHTTP、BITS、OkHttp 和 `DownloadManager`，不为更新引入浏览器内核或大型运行时依赖。
- 正式版本不可用相同版本号覆盖；版本和发布规则继续遵循 [发布版本规范](release-versioning.md)。

第一版不包含更新通道选择、灰度发布、增量补丁、强制最低版本、回滚按钮、国内镜像或更新统计。

当前实现状态：

| 能力 | 状态 |
| --- | --- |
| Git 标签触发 GitHub Actions | 已实现 |
| Android 签名 APK 自动构建与校验 | 已实现 |
| Windows 程序、TSF DLL 和 Inno Setup 安装包自动构建、签名与校验 | 已实现 |
| GitHub Release 按平台独立创建并上传产物 | 已实现 |
| `flowtype-update.json`、独立清单签名和草稿完成后发布 | 已实现 |
| Windows 和 Android 客户端检查、后台下载与安装交互 | 已实现 |

## 2. 发布与发现拓扑

```text
平台版本标签 windows-vX.Y.Z / android-vX.Y.Z
       |
       v
GitHub Actions
  |-- 目标平台构建并签名产物
  |-- 生成 SHA-256 和更新清单
  |-- 使用独立更新私钥签署清单
       |
       v
GitHub Release（每个平台独立，先草稿，资产齐全后发布）
       |
       +-- flowtype-update.json
       +-- flowtype-update.json.sig
       +-- FlowType-<version>-x64-setup.exe
       +-- FlowType-<version>-android-release.apk
       +-- SHA-256 / Authenticode 验证记录
                  |
                  +-- Windows：WinHTTP 检查，BITS 下载
                  +-- Android：OkHttp 检查，DownloadManager 下载
```

生产客户端先读取 GitHub Releases API，按平台筛选最新的 `windows-vX.Y.Z` 或 `android-vX.Y.Z` 正式 Release，再下载该 Release 中的清单和签名。客户端不需要 GitHub Token。

GitHub Release 必须先创建为草稿，所有安装包、校验文件、清单和签名上传并复核完成后再发布。这样 `latest` 不会短暂指向资产不完整的版本。

## 3. 更新清单

`flowtype-update.json` 使用 UTF-8、无 BOM 的 JSON。平台独立发布使用 schema 2，每份清单只包含对应平台资产：

```json
{
  "schema": 2,
  "key_id": "flowtype-update-2026-v2",
  "platform": "android",
  "version": "0.1.19",
  "published_at": "2026-08-26T10:00:00Z",
  "release_url": "https://github.com/Henry10088/FlowType/releases/tag/android-v0.1.19",
  "notes_zh_cn": "修复实时输入稳定性问题。",
  "android": {
    "version_code": 20,
    "url": "https://github.com/Henry10088/FlowType/releases/download/android-v0.1.19/FlowType-0.1.19-android-release.apk",
    "sha256": "0000000000000000000000000000000000000000000000000000000000000000",
    "size": 2345678
  }
}
```

约束如下：

- Windows 使用严格 SemVer 比较 `version`；Android 以严格递增的 `version_code` 判断是否可升级。两端版本可以不同。
- 客户端只接受受支持的 `schema`、HTTPS 下载地址、大于零且在实现上限内的 `size` 和格式正确的 SHA-256。示例中的全零摘要只是占位值，发布清单必须填入产物真实摘要。
- 相同或更低版本不提示安装，正式客户端不自动降级。
- `release_url` 只用于用户查看发布说明，不能替代安装包校验。
- 清单字段扩展必须保持旧客户端可忽略；破坏性变化提升 `schema`。

## 4. 清单签名与发布密钥

HTTPS 负责传输加密，但更新决策不能只信任网络地址。GitHub Actions 对 `flowtype-update.json` 的原始字节生成 ECDSA P-256 / SHA-256 分离签名；`flowtype-update.json.sig` 保存 DER 签名的 Base64 文本。客户端必须先验证原始清单签名，再解析和使用其中的 URL、版本与摘要。

更新签名使用独立密钥，不复用设备绑定私钥、Android Release Keystore 或 Windows Authenticode 证书。验证公钥内置在两端客户端中，它不是秘密。更换更新签名密钥时，必须先发布同时信任新旧公钥的过渡版本。

GitHub 仓库使用受保护的 `release` Environment，并为正式发布设置人工批准。现有发布 Secrets 为：

- `FLOWTYPE_ANDROID_KEYSTORE_BASE64`
- `FLOWTYPE_ANDROID_STORE_PASSWORD`
- `FLOWTYPE_ANDROID_KEY_ALIAS`
- `FLOWTYPE_ANDROID_KEY_PASSWORD`
- `FLOWTYPE_WINDOWS_CERT_BASE64`
- `FLOWTYPE_WINDOWS_CERT_PASSWORD`

在线更新增加：

- `FLOWTYPE_UPDATE_SIGNING_KEY_BASE64`

私钥只在 GitHub Actions 临时运行环境中解码，不输出到日志、不进入构建产物和 Git。Android Keystore、更新签名私钥和 Windows 证书必须保留加密的离线备份。丢失 Android Keystore 后，已安装 APK 无法再通过正常覆盖安装升级。

## 5. 检查策略

- 应用启动后延迟约 30 秒检查，避免与启动、自动连接和首次输入争抢网络与界面注意力。
- 自动检查成功后 24 小时内不重复请求；手工“检查更新”不受此限制。
- 只有清单和签名都下载成功、签名有效且字段合法，才记录本次成功检查时间。
- 请求失败、超时、GitHub 不可达或返回限流时保持当前版本正常运行，不弹出阻断对话框。
- 自动检查发现新版本后只在设置页和 Windows 托盘菜单显示轻量提示，不抢焦点、不打断语音输入。
- 手工检查需要在原位置显示检查中、最新版本、新版本或具体失败状态。

Windows 使用系统 WinHTTP 获取体积很小的 Releases API 响应、清单和签名；Android 复用现有 OkHttp。两端按平台筛选正式标签，并设置合理的连接和读取超时。

## 6. Windows 更新流程

### 6.1 下载

Windows 使用 Background Intelligent Transfer Service（BITS）下载安装包：

- BITS 在后台传输，不占用 Win32 UI、WSS 服务或文本注入线程。
- BITS 作业可跨主程序退出和系统重启继续；客户端持久化作业 ID，并在再次启动时重新关联。
- BITS 根据网络和服务器能力执行重试与断点续传。GitHub Release 资产不可被同版本覆盖，避免恢复时拼接到不同文件。
- 设置页显示 `正在下载 42% · 1.3/3.2 MB`；未知总大小时显示已下载字节，不伪造百分比。
- 用户可取消本次下载；网络等待显示“等待网络”，不把正常重试表现为连续闪烁的错误。

若本机 BITS 被策略禁用或服务不可用，显示明确错误和“重试”“打开发布页”，第一版不增加自研下载兜底。

### 6.2 校验与安装

下载完成后依次验证：

1. 更新清单的 P-256 分离签名。
2. 安装包文件长度与清单一致。
3. 安装包 SHA-256 与清单一致。
4. Authenticode 签名有效，签名发布者身份与 FlowType 正式证书匹配。

任何一步失败都删除不可用的临时文件并停止安装，当前版本继续工作。

校验成功后显示“更新已下载”和“安装更新”。活动文字会话期间允许继续下载，但不启动安装；按钮显示“输入结束后可安装”。用户确认后，主程序启动已签名的 Inno Setup 安装包并正常退出，安装程序申请 UAC、执行覆盖升级并保留绑定和设置。安装取消或失败时旧版本仍可继续使用。

## 7. Android 更新流程

### 7.1 下载

Android 使用系统 `DownloadManager` 下载 APK：

- 下载由系统在后台执行，可跨 Activity、应用进程和设备重启继续。
- 使用系统下载通知，同时在应用设置页显示与 Windows 一致的进度和状态。
- 应用保存下载 ID，恢复后通过系统查询当前状态，不重复创建同一版本任务。
- 系统根据 GitHub 的 HTTP Range 支持恢复下载；远端状态不再满足恢复条件时，由系统重新下载完整 APK，不由应用拼接文件。

### 7.2 校验与安装

下载完成后验证清单签名、文件长度、APK SHA-256 和 APK 签名证书是否与当前正式应用一致，再通过系统包安装器安装。Android 不允许普通应用静默更新，用户必须确认系统安装界面。

通过 GitHub APK 首次更新时，系统可能要求用户允许说写“安装未知应用”。只在用户点击“安装更新”后说明用途并进入对应系统设置，不在输入过程中突然申请。后续 APK 必须保持相同的 applicationId 和同一 Android Release Keystore 签名，否则系统会拒绝覆盖安装。

活动文字会话期间可以下载，但“安装更新”提示用户先结束当前会话，避免包安装导致应用进程退出和草稿状态中断。

## 8. 用户界面状态

Android 和 Windows 设置页都增加紧凑的“版本与更新”区域，不增加独立首页或复杂更新中心。

| 状态 | 主要文案 | 可用操作 |
| --- | --- | --- |
| 空闲 | `当前版本 0.2.0` | `检查更新` |
| 检查中 | `正在检查更新…` | 无 |
| 已是最新 | `已是最新版本` | `再次检查` |
| 发现更新 | `发现新版本 0.1.19` | `下载更新` |
| 下载中 | `正在下载 42% · 1.3/3.2 MB` | `取消` |
| 网络等待 | `等待网络，恢复后继续下载` | `取消` |
| 校验中 | `正在校验更新…` | 无 |
| 已下载 | `更新已下载` | `安装更新` |
| 会话活跃 | `输入结束后可安装` | 无 |
| 失败 | 说明检查、下载或校验失败的具体原因 | `重试`、`打开发布页` |

进度更新只替换原位置文字和进度条，不弹 Toast、不抢焦点、不让页面闪烁。关闭设置页不取消下载。更新提示不使用大面积强调色；安装是明确命令，使用普通现代按钮。

## 9. 失败处理与数据保留

- GitHub 在中国网络环境中访问较慢或暂时不可用时，在线更新退化为稍后重试，实时输入完全不依赖 GitHub。
- 检查失败不清除上一次已知的新版本信息；界面同时标明本次检查失败，不能把缓存结果冒充刚刚获取。
- 下载中断交给 BITS 或 DownloadManager 恢复，不在业务层拆分分片或维护下载事务日志。
- 用户取消后清理对应系统下载任务和临时文件，不删除当前安装。
- 摘要、平台签名或清单签名失败视为安全错误，不提供“仍然安装”。
- 新版本安装不迁移或删除绑定、Android 历史和用户设置；需要 schema 迁移时必须前向迁移。

## 10. GitHub Actions 发布流程

`.github/workflows/release.yml` 已在现有发布流水线上实现以下步骤：

1. 校验 Git 标签、Windows/Android/安装包版本以及 Android `versionCode`。
2. 并行构建两端正式产物，完成 Android APK 签名和 Windows Authenticode 签名。
3. 创建草稿 Release，上传安装包和现有校验记录。
4. 从最终产物计算文件大小和 SHA-256，生成 `flowtype-update.json`。
5. 在受保护的 `release` Environment 中读取独立更新签名私钥，签署清单原始字节。
6. 上传清单和分离签名，并在干净步骤中用公开验证密钥复核签名、摘要、版本和资产 URL。
7. 所有检查通过后发布目标平台 Release；任一步失败都保留为草稿，不影响另一平台的发布。

正式 Release 仍需项目所有者明确授权推送版本标签。工作流不从分支普通 push 自动发布，也不能把测试 prerelease 变成生产客户端的 latest。

## 11. 验收条件

1. 两端可以自动和手工发现一个更高的正式版本，相同版本和 prerelease 不提示。
2. 清单被修改、签名不匹配、包体被修改、SHA-256 不匹配、APK 签名证书或 Authenticode 身份不匹配时均拒绝安装。
3. Windows 下载期间关闭应用或重启电脑后，BITS 作业能够重新关联并继续。
4. Android 下载期间退出应用或重启手机后，DownloadManager 任务能够重新关联并继续。
5. 下载进度、网络等待、校验、可安装和失败状态在原位置稳定显示。
6. 检查和下载期间，实时文字输入、自动重连、图片和文件传输不被阻塞或明显降速。
7. 活跃文字会话期间不启动安装；结束会话后可由用户确认安装。
8. Windows 覆盖升级保留绑定和设置；Android 覆盖升级保留绑定、草稿、历史和设置。
9. GitHub 不可访问、下载取消或安装取消时，当前版本仍可正常使用。
10. Release 只有在所有签名、摘要、清单和资产上传完成后才从草稿发布。

## 12. 后续扩展

如果 GitHub 在目标网络中的下载体验长期不满足要求，再评估国内对象存储与 CDN。镜像必须托管同一份已签名清单和按 SHA-256 固定的产物，客户端继续以签名和摘要建立信任，不能因为使用 CDN 而降低校验。第一版不为尚未启用的镜像增加配置页和运行时分支。
