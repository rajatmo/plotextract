# Build the Windows installer (MSI) with WiX: <Out>\plotextract-<version>-<platform>.msi.
# It runs dist.ps1 first, so the release app, its folder and its zip are made too.
#
#   scripts\installer.ps1                 # writes to ..\dist, next to the repository
#   scripts\installer.ps1 -Out D:\release
#
# Needs WiX 5 as a .NET tool:  dotnet tool install --global wix --version 5.0.2
# The dialogs come from its UI extension, which this script adds if it is missing.
param([string]$Out = (Join-Path (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)) 'dist'))

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

if (-not (Get-Command wix -ErrorAction SilentlyContinue)) {
    throw 'WiX is not installed: dotnet tool install --global wix --version 5.0.2'
}
$wix_version = ((wix --version) -split '\+')[0]

& (Join-Path $PSScriptRoot 'dist.ps1') -Out $Out
Set-Location $repo
$Out = (Resolve-Path $Out).Path

$meta = cargo metadata --no-deps --format-version 1 | ConvertFrom-Json
$version = ($meta.packages | Where-Object name -eq 'plotextract').version
$host_triple = (rustc -vV | Select-String '^host: (.+)$').Matches[0].Groups[1].Value
$platform, $arch = switch -Regex ($host_triple) {
    '^x86_64-.*windows' { 'windows-x86_64', 'x64' }
    '^aarch64-.*windows' { 'windows-aarch64', 'arm64' }
    default { throw "the installer is for Windows; the host is $host_triple" }
}

# the extension must be of the same version as WiX itself
$ui = "WixToolset.UI.wixext/$wix_version"
if (-not ((wix extension list -g) -match [regex]::Escape("WixToolset.UI.wixext $wix_version"))) {
    wix extension add -g $ui
    if ($LASTEXITCODE -ne 0) { throw "could not add the WiX extension $ui" }
}

$msi = Join-Path $Out "plotextract-$version-$platform.msi"
if (Test-Path $msi) { Remove-Item -Force $msi }
wix build (Join-Path $repo 'wix\plotextract.wxs') `
    -arch $arch -ext $ui `
    -d "Version=$version" -d "Source=$(Join-Path $Out $platform)" -d "Repo=$repo" `
    -pdbtype none -o $msi
if ($LASTEXITCODE -ne 0) { throw 'wix build failed' }

Get-Item $msi | Select-Object Name, @{n = 'MB'; e = { [math]::Round($_.Length / 1MB, 1) }}
Write-Host "wrote $msi"
