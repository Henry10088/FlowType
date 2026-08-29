[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string] $InputPath,

    [Parameter(Mandatory = $true)]
    [string] $OutputPath
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path -LiteralPath $InputPath -PathType Leaf)) {
    throw "Android Gradle lockfile was not found: $InputPath"
}

$runtimeLines = @(
    Get-Content -LiteralPath $InputPath |
        Where-Object { $_ -match "releaseRuntimeClasspath" }
)

if ($runtimeLines.Count -eq 0) {
    throw "No releaseRuntimeClasspath entries were found in: $InputPath"
}

$outputDirectory = Split-Path -Parent $OutputPath
if ($outputDirectory) {
    New-Item -ItemType Directory -Force -Path $outputDirectory | Out-Null
}

$content = @(
    "# Generated for OSV-Scanner: Android release runtime dependencies only."
    $runtimeLines
)
$content | Set-Content -LiteralPath $OutputPath -Encoding utf8NoBOM

Write-Host "Created Android runtime lockfile with $($runtimeLines.Count) entries: $OutputPath"
