# Package a release build for Windows.
# Usage: ./package-windows.ps1 -Version v0.1.0 -Target x86_64-pc-windows-msvc -Name windows-x86_64
# Run from the repository root after: cargo build --release -p kalem --target <target>
param(
  [Parameter(Mandatory)][string]$Version,
  [Parameter(Mandatory)][string]$Target,
  [Parameter(Mandatory)][string]$Name
)
$ErrorActionPreference = "Stop"

$pkg = "kalem-ide-$Version-$Name"
$bin = "target/$Target/release/kalem.exe"
if (-not (Test-Path $bin)) { throw "missing $bin - build first" }

Remove-Item -Recurse -Force $pkg, "$pkg.zip", "$pkg.zip.sha256" -ErrorAction SilentlyContinue
New-Item -ItemType Directory $pkg | Out-Null
Copy-Item $bin $pkg/
Copy-Item -Recurse src/kalem-lang/languages "$pkg/languages"
Copy-Item README.md, LICENSE-MIT, LICENSE-APACHE $pkg/
Compress-Archive -Path $pkg -DestinationPath "$pkg.zip"
(Get-FileHash "$pkg.zip" -Algorithm SHA256).Hash.ToLower() + "  $pkg.zip" | Out-File "$pkg.zip.sha256" -Encoding ascii
Remove-Item -Recurse -Force $pkg
Write-Host "created $pkg.zip"
