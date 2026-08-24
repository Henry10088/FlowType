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

发布前使用 Android SDK `apksigner verify --verbose --print-certs` 验证签名，并单独保存证书 SHA-256 指纹。

## Windows

使用受信任代码签名证书依次签署：

1. `flowtype.exe`
2. `flowtype-injector.exe`
3. 重新编译 Inno Setup 安装包
4. `FlowType-<version>-x64-setup.exe`

必须使用 RFC 3161 时间戳服务和 SHA-256 摘要。发布前用 PowerShell `Get-AuthenticodeSignature` 验证三个文件均为 `Valid`，并检查签名主体与项目发布者一致。

Windows 安装包内的两个程序应先签名，安装包在最后签名；不能只签最外层安装包。
