param(
    [string]$ExpectedTag = "",
    [ValidateSet("All", "Windows", "Android")]
    [string]$Platform = "All",
    [switch]$Development
)

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
$androidZhLabelVersion = Read-RequiredMatch "android/app/src/main/res/values-zh/strings.xml" 'name="version_label">[^0-9]*([0-9]+\.[0-9]+\.[0-9]+)' "Android Chinese displayed version"
$installerVersion = Read-RequiredMatch "installer/flowtype.iss" '^#define AppVersion "([^" ]+)"' "installer version"
$readmeVersion = Read-RequiredMatch "README.md" '^.*?([0-9]+\.[0-9]+\.[0-9]+)' "README version"
$readmeZhVersion = Read-RequiredMatch "README.zh-CN.md" '^.*?([0-9]+\.[0-9]+\.[0-9]+)' "Chinese README version"
$latestTagPattern = switch ($Platform) {
    "Windows" { "windows-v[0-9]*" }
    "Android" { "android-v[0-9]*" }
    default { "v[0-9]*" }
}
$latestTag = ((& git -C $root tag --list $latestTagPattern --sort=-version:refname | Select-Object -First 1) -as [string]).Trim()
$previousTagPattern = switch ($Platform) {
    "Windows" { '^windows-v(\d+\.\d+\.\d+)$' }
    "Android" { '^android-v(\d+\.\d+\.\d+)$' }
    default { '^v(\d+\.\d+\.\d+)$' }
}
$applicationVersion = if ($Platform -eq "Android") { $androidVersion } else { $cargoVersion }
$exactTag = ""
try {
    $exactTag = (& git -C $root describe --tags --exact-match HEAD 2>$null).Trim()
} catch {
    # An untagged development commit is the normal state before publishing.
}

if ($Platform -eq "Windows") {
    if ($installerVersion -ne $cargoVersion) {
        throw "Windows version mismatch: Cargo=$cargoVersion, Installer=$installerVersion"
    }
} elseif ($Platform -eq "Android") {
    if ($androidLabelVersion -ne $androidVersion -or $androidZhLabelVersion -ne $androidVersion) {
        throw "Android displayed version mismatch: versionName=$androidVersion, English=$androidLabelVersion, Chinese=$androidZhLabelVersion"
    }
} else {
    $versions = @{
        "Cargo" = $cargoVersion
        "Android" = $androidVersion
        "Android displayed" = $androidLabelVersion
        "Android Chinese displayed" = $androidZhLabelVersion
        "Installer" = $installerVersion
    }
    $mismatches = $versions.GetEnumerator() | Where-Object { $_.Value -ne $cargoVersion }
    if ($mismatches) {
        $details = ($versions.GetEnumerator() | ForEach-Object { "$($_.Key)=$($_.Value)" }) -join ", "
        throw "Version mismatch: $details"
    }
}

if ([int]$androidCode -lt 1) {
    throw "Android versionCode must be positive"
}

if ($ExpectedTag) {
    $tagPattern = switch ($Platform) {
        "Windows" { '^windows-v([0-9]+\.[0-9]+\.[0-9]+)(-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?$' }
        "Android" { '^android-v([0-9]+\.[0-9]+\.[0-9]+)(-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?$' }
        default { '^v([0-9]+\.[0-9]+\.[0-9]+)(-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?$' }
    }
    if ($ExpectedTag -notmatch $tagPattern) {
        throw "Release tag is not valid for ${Platform}: $ExpectedTag"
    }
    $tagVersion = $Matches[1]
    $prerelease = $Matches[2]
    if ($tagVersion -ne $applicationVersion) {
        throw "Release tag $ExpectedTag does not match $Platform version $applicationVersion"
    }
    if ($exactTag -and $exactTag -ne $ExpectedTag) {
        throw "HEAD is tagged as $exactTag, not expected release tag $ExpectedTag"
    }

    $stableTagPattern = switch ($Platform) {
        "Windows" { '^windows-v[0-9]+\.[0-9]+\.[0-9]+$' }
        "Android" { '^android-v[0-9]+\.[0-9]+\.[0-9]+$' }
        default { '^v[0-9]+\.[0-9]+\.[0-9]+$' }
    }
    $stableTags = @(& git -C $root tag --list $latestTagPattern --sort=-version:refname) |
        Where-Object { $_ -match $stableTagPattern -and $_ -ne $ExpectedTag }
    $previousStableTag = $stableTags | Select-Object -First 1
    if ($previousStableTag -and $previousStableTag -match $previousTagPattern) {
        $previousVersion = [Version]$Matches[1]
        if ($prerelease) {
            if ([Version]$applicationVersion -lt $previousVersion) {
                throw "Prerelease version $cargoVersion is older than $previousStableTag"
            }
        } elseif ([Version]$applicationVersion -le $previousVersion) {
            throw "Stable version $applicationVersion must be greater than $previousStableTag"
        }

        if (!$prerelease -and $Platform -eq "Android") {
            $previousGradle = (& git -C $root show "${previousStableTag}:android/app/build.gradle.kts") -join "`n"
            $previousCodeMatch = [regex]::Match($previousGradle, '^\s*versionCode\s*=\s*(\d+)', [Text.RegularExpressions.RegexOptions]::Multiline)
            if (!$previousCodeMatch.Success) {
                throw "Could not read Android versionCode from $previousStableTag"
            }
            $previousCode = [int]$previousCodeMatch.Groups[1].Value
            if ([int]$androidCode -le $previousCode) {
                throw "Android versionCode $androidCode must be greater than $previousCode from $previousStableTag"
            }
        }
    }
}

if (!$Development -and $latestTag -and $exactTag -ne $latestTag -and $latestTag -match $previousTagPattern) {
    $latestReleasedVersion = [Version]$Matches[1]
    if ([Version]$applicationVersion -le $latestReleasedVersion) {
        throw "Version $applicationVersion must be greater than latest released version $latestTag before building a new distributable."
    }
}

$tagMessage = if ($ExpectedTag) { "; release tag=$ExpectedTag" } else { "" }
Write-Output "FlowType $Platform version is consistent; Windows=$cargoVersion, Android=$androidVersion (versionCode=$androidCode)$tagMessage."
