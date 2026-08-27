param(
    [Parameter(Mandatory = $true)][string]$Tag,
    [Parameter(Mandatory = $true)][ValidateSet("windows", "android")][string]$Platform,
    [Parameter(Mandatory = $true)][string]$Version,
    [int]$AndroidVersionCode = 0,
    [string]$WindowsInstaller = "",
    [string]$AndroidApk = "",
    [Parameter(Mandatory = $true)][string]$PublicKey,
    [Parameter(Mandatory = $true)][string]$OutputDirectory
)

$ErrorActionPreference = "Stop"
$keyId = "flowtype-update-2026-v2"
$repository = "Henry10088/FlowType"

if ($Tag -notmatch '^(windows|android)-v[0-9]+\.[0-9]+\.[0-9]+$' -or $Tag -notmatch "^$Platform-") {
    throw "Invalid $Platform release tag: $Tag"
}
if ($Version -notmatch '^[0-9]+\.[0-9]+\.[0-9]+$' -or $Tag -ne "$Platform-v$Version") {
    throw "Release tag $Tag does not match version $Version"
}
if ($Platform -eq "android" -and $AndroidVersionCode -lt 1) {
    throw "Android versionCode must be positive"
}
if ($Platform -eq "windows" -and [string]::IsNullOrWhiteSpace($WindowsInstaller)) {
    throw "Windows installer is required for a Windows release"
}
if ($Platform -eq "android" -and [string]::IsNullOrWhiteSpace($AndroidApk)) {
    throw "Android APK is required for an Android release"
}
$requiredPaths = @($PublicKey)
if ($Platform -eq "windows") { $requiredPaths += $WindowsInstaller }
if ($Platform -eq "android") { $requiredPaths += $AndroidApk }
foreach ($path in $requiredPaths) {
    if (!(Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Required update input does not exist: $path"
    }
}

$privateText = $env:FLOWTYPE_UPDATE_SIGNING_KEY_BASE64
if ([string]::IsNullOrWhiteSpace($privateText)) {
    throw "Missing FLOWTYPE_UPDATE_SIGNING_KEY_BASE64"
}
$privateBytes = [Convert]::FromBase64String($privateText.Trim())
$publicBytes = [Convert]::FromBase64String(([IO.File]::ReadAllText($PublicKey)).Trim())

$signer = [Security.Cryptography.ECDsa]::Create()
$verifier = [Security.Cryptography.ECDsa]::Create()
try {
    $read = 0
    $signer.ImportPkcs8PrivateKey($privateBytes, [ref]$read)
    if ($read -ne $privateBytes.Length) {
        throw "Update signing key contains trailing data"
    }
    if (![Security.Cryptography.CryptographicOperations]::FixedTimeEquals(
        $signer.ExportSubjectPublicKeyInfo(),
        $publicBytes
    )) {
        throw "Update signing key does not match release/update-public-key-spki.b64"
    }
    $read = 0
    $verifier.ImportSubjectPublicKeyInfo($publicBytes, [ref]$read)
    if ($read -ne $publicBytes.Length) {
        throw "Update public key contains trailing data"
    }

    $windows = if ($Platform -eq "windows") { Get-Item -LiteralPath $WindowsInstaller } else { $null }
    $android = if ($Platform -eq "android") { Get-Item -LiteralPath $AndroidApk } else { $null }
    $baseUrl = "https://github.com/$repository/releases/download/$Tag"
    $manifest = [ordered]@{
        schema = 2
        key_id = $keyId
        platform = $Platform
        version = $Version
        published_at = [DateTimeOffset]::UtcNow.ToString("yyyy-MM-dd'T'HH:mm:ss'Z'")
        release_url = "https://github.com/$repository/releases/tag/$Tag"
        notes_zh_cn = "查看 GitHub Release 获取本次更新说明。"
        windows = [ordered]@{
            url = ""
            sha256 = ""
            size = 0
        }
        android = [ordered]@{
            version_code = 0
            url = ""
            sha256 = ""
            size = 0
        }
    }
    if ($Platform -eq "windows") {
        $manifest.windows = [ordered]@{
            url = "$baseUrl/$($windows.Name)"
            sha256 = (Get-FileHash -LiteralPath $windows.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
            size = $windows.Length
        }
    } else {
        $manifest.android = [ordered]@{
            version_code = $AndroidVersionCode
            url = "$baseUrl/$($android.Name)"
            sha256 = (Get-FileHash -LiteralPath $android.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
            size = $android.Length
        }
    }

    New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
    $manifestPath = Join-Path $OutputDirectory "flowtype-update.json"
    $signaturePath = Join-Path $OutputDirectory "flowtype-update.json.sig"
    $json = $manifest | ConvertTo-Json -Depth 4
    $utf8 = [Text.UTF8Encoding]::new($false)
    [IO.File]::WriteAllText($manifestPath, $json, $utf8)
    $manifestBytes = [IO.File]::ReadAllBytes($manifestPath)
    if ($manifestBytes.Length -gt 65536) {
        throw "Update manifest exceeds 64 KiB"
    }
    $signature = $signer.SignData(
        $manifestBytes,
        [Security.Cryptography.HashAlgorithmName]::SHA256,
        [Security.Cryptography.DSASignatureFormat]::Rfc3279DerSequence
    )
    if (!$verifier.VerifyData(
        $manifestBytes,
        $signature,
        [Security.Cryptography.HashAlgorithmName]::SHA256,
        [Security.Cryptography.DSASignatureFormat]::Rfc3279DerSequence
    )) {
        throw "Generated update signature did not verify"
    }
    [IO.File]::WriteAllText($signaturePath, [Convert]::ToBase64String($signature), $utf8)
    Write-Output "Created signed update manifest for $Tag"
} finally {
    $signer.Dispose()
    $verifier.Dispose()
    [Security.Cryptography.CryptographicOperations]::ZeroMemory($privateBytes)
}
