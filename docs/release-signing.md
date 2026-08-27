# V1 发布签名配置

签名材料只保存在发布机器，不提交 Git。没有项目所有者提供的正式证书时，构建产物只能用于内部验证，不能标记为正式发布。

## Android

设置以下四个环境变量后运行 `android\gradlew.bat assembleRelease`：

```text
FLOWTYPE_ANDROID_KEYSTORE
FLOWTYPE_ANDROID_STORE_PASSWORD
FLOWTYPE_ANDROID_KEY_ALIAS
FLOWTYPE_ANDROID_KEY_PASSWORD
```

四项必须同时存在；只配置部分值时构建会立即失败。未配置任何一项时仍可生成明确标记为 unsigned 的内部测试 APK。

发布前使用 Android SDK `apksigner verify --verbose --print-certs` 验证签名，并单独保存证书 SHA-256 指纹。GitHub Actions 会始终验证 APK 签名有效；如需固定发布证书，还可在 `release` Environment Variables 中配置 `FLOWTYPE_ANDROID_CERT_SHA256`，不配置时不会阻断构建。

## Windows

使用受信任代码签名证书依次签署：

1. `flowtype.exe`
2. `flowtype-injector.exe`
3. `flowtype_tip.dll`
4. `flowtype_tip_x86.dll`
5. 重新编译 Inno Setup 安装包
6. `FlowType-<version>-x64-setup.exe`

必须使用 RFC 3161 时间戳服务和 SHA-256 摘要。发布前用 PowerShell `Get-AuthenticodeSignature` 验证以上文件均为 `Valid`，并检查签名主体与项目发布者一致。GitHub Actions 会始终验证 Authenticode 签名有效；如需固定发布证书，还可配置 `FLOWTYPE_WINDOWS_CERT_SHA256`，不配置时不会阻断构建。

Windows 安装包内的两个程序和两个 TIP DLL 应先签名，安装包在最后签名；不能只签最外层安装包。

## 在线更新清单

更新清单使用独立的 ECDSA P-256 私钥生成 SHA-256 分离签名，不复用 Android Release Keystore、Windows Authenticode 证书或设备绑定密钥。验证公钥编译进 Windows 和 Android 客户端；当前密钥 ID 为 `flowtype-update-2026-v2`。私钥以 `FLOWTYPE_UPDATE_SIGNING_KEY_BASE64` 保存到受保护的 GitHub `release` Environment，并保留加密离线备份。

GitHub Actions 必须对 `flowtype-update.json` 的原始字节签名，上传 Base64 编码的 DER 签名 `flowtype-update.json.sig`。Release 发布前要在干净步骤中用公开验证密钥复核签名。完整流程见 [GitHub 在线更新设计基线](update-design-v1.md)。
