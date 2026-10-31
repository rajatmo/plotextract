# Build the release app and put it in <Out>\<platform>\, with a zip beside it. Run on the platform you want to build for.
#
#   scripts\dist.ps1                 # writes to ..\dist, next to the repository
#   scripts\dist.ps1 -Out D:\release
param([string]$Out = (Join-Path (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)) 'dist'))

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo

cargo build --release -p plotextract
if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }

$meta = cargo metadata --no-deps --format-version 1 | ConvertFrom-Json
$version = ($meta.packages | Where-Object name -eq 'plotextract').version
$host_triple = (rustc -vV | Select-String '^host: (.+)$').Matches[0].Groups[1].Value
$platform = switch -Regex ($host_triple) {
    '^x86_64-.*windows' { 'windows-x86_64' }
    '^aarch64-.*windows' { 'windows-aarch64' }
    default { throw "this script builds for Windows; the host is $host_triple (use scripts/dist.sh)" }
}

$dest = Join-Path $Out $platform
if (Test-Path $dest) { Remove-Item -Recurse -Force $dest }
New-Item -ItemType Directory -Force $dest | Out-Null
Copy-Item (Join-Path $meta.target_directory 'release\plotextract.exe') $dest
Copy-Item (Join-Path $PSScriptRoot 'README.dist.txt') (Join-Path $dest 'README.txt')
Copy-Item (Join-Path $repo 'LICENSE-MIT'), (Join-Path $repo 'LICENSE-APACHE') $dest

$zip = Join-Path $Out "plotextract-$version-$platform.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
Compress-Archive -Path (Join-Path $dest '*') -DestinationPath $zip

Get-ChildItem $dest | Select-Object Name, @{n = 'MB'; e = { [math]::Round($_.Length / 1MB, 1) }}
Write-Host "wrote $dest and $zip"
