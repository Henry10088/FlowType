$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot

function Read-RequiredMatch([string]$Path, [string]$Pattern, [string]$Name) {
    $text = [IO.File]::ReadAllText((Join-Path $root $Path))
    $match = [regex]::Match($text, $Pattern, [Text.RegularExpressions.RegexOptions]::Multiline)
    if (!$match.Success) {
        throw "Could not read $Name from $Path"
    }
    return $match.Groups[1].Value
}

$cargoVersion = Read-RequiredMatch "windows/Cargo.toml" '^version\s*=\s*"([^" ]+)"' "Cargo workspace version"
$androidVersion = Read-RequiredMatch "android/app/build.gradle.kts" '^\s*versionName\s*=\s*"([^" ]+)"' "Android versionName"
$androidCode = Read-RequiredMatch "android/app/build.gradle.kts" '^\s*versionCode\s*=\s*(\d+)' "Android versionCode"
$androidLabelVersion = Read-RequiredMatch "android/app/src/main/res/values/strings.xml" 'name="version_label">[^0-9]*([0-9]+\.[0-9]+\.[0-9]+)' "Android displayed version"
$installerVersion = Read-RequiredMatch "installer/flowtype.iss" '^#define AppVersion "([^" ]+)"' "installer version"
$readmeVersion = Read-RequiredMatch "README.md" '^.*?([0-9]+\.[0-9]+\.[0-9]+)' "README version"
$releaseDocVersion = Read-RequiredMatch "docs/release-versioning.md" '^.{0,10}([0-9]+\.[0-9]+\.[0-9]+).*Android' "release document version"

$versions = @{
    "Cargo" = $cargoVersion
    "Android" = $androidVersion
    "Android displayed" = $androidLabelVersion
    "Installer" = $installerVersion
    "README" = $readmeVersion
    "Release document" = $releaseDocVersion
}
$mismatches = $versions.GetEnumerator() | Where-Object { $_.Value -ne $cargoVersion }
if ($mismatches) {
    $details = ($versions.GetEnumerator() | ForEach-Object { "$($_.Key)=$($_.Value)" }) -join ", "
    throw "Version mismatch: $details"
}

if ([int]$androidCode -lt 1) {
    throw "Android versionCode must be positive"
}

Write-Output "FlowType version $cargoVersion is consistent; Android versionCode=$androidCode."
