<#
.SYNOPSIS
  Build, sign and package Kalem IDE for Windows.

.DESCRIPTION
  Produces, in $env:DIST (default: dist/release):
    kalem-ide-<version>-windows-<arch>.msi            per-machine installer (WiX v5), adds kalem to PATH
    kalem-ide-<version>-windows-<arch>-portable.zip   kalem.exe + languages\ (unzip and run)
    plus a .sha256 beside each

  The binary links the C runtime statically (+crt-static), so neither package needs the Visual
  C++ redistributable. Signing is delegated to sign.ps1 (skipped with a warning without secrets).

  Needs: Rust (MSVC toolchain + the target), the Windows SDK (signtool.exe) and WiX v5:
    dotnet tool install --global wix --version 5.0.2

.EXAMPLE
  pwsh deployments/windows/package.ps1 -Arch x64
  pwsh deployments/windows/package.ps1 -Arch arm64   # cross-compiled; needs the MSVC ARM64 build tools
#>
param(
  [ValidateSet('x64', 'arm64')] [string] $Arch = 'x64',
  [switch] $SkipBuild
)
$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path

function Invoke-Native([string] $What, [scriptblock] $Block) {
  Write-Output "==> $What"
  & $Block
  if ($LASTEXITCODE -ne 0) { throw "$What failed with exit code $LASTEXITCODE" }
}

# The version lives in one place: [workspace.package] version in the root Cargo.toml.
$Version = if ($env:KALEM_VERSION) { $env:KALEM_VERSION.TrimStart('v') } else { $null }
if (-not $Version) {
  $inPkg = $false
  foreach ($line in Get-Content (Join-Path $Root 'Cargo.toml')) {
    if ($line -match '^\s*\[') { $inPkg = ($line.Trim() -eq '[workspace.package]'); continue }
    if ($inPkg -and $line -match '^\s*version\s*=\s*"([^"]+)"') { $Version = $Matches[1]; break }
  }
}
if (-not $Version) { throw 'could not read [workspace.package] version from Cargo.toml' }
# MSI ProductVersion is numeric (major.minor.build); pre-release tags are dropped there.
$MsiVersion = ($Version -split '-')[0]

$Target = switch ($Arch) { 'x64' { 'x86_64-pc-windows-msvc' } 'arm64' { 'aarch64-pc-windows-msvc' } }
$Dist = if ($env:DIST) { $env:DIST } else { Join-Path $Root 'dist\release' }
$TargetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $Root 'target' }
$LangDir = Join-Path $Root 'src\kalem-lang\languages'
New-Item -ItemType Directory -Force -Path $Dist | Out-Null
$Name = "kalem-ide-$Version-windows-$Arch"

Write-Output "Kalem IDE $Version for Windows $Arch ($Target)"

if (-not $SkipBuild) {
  # Static CRT: no VC++ redistributable needed. Scoped to the target so host build scripts and
  # proc-macros are unaffected.
  $flagVar = 'CARGO_TARGET_' + ($Target.ToUpper() -replace '-', '_') + '_RUSTFLAGS'
  [Environment]::SetEnvironmentVariable($flagVar, '-C target-feature=+crt-static')
  Invoke-Native "cargo build ($Target)" { cargo build --release --locked -p kalem --target $Target }
}

$Bin = Join-Path $TargetDir "$Target\release"
$Exe = Join-Path $Bin 'kalem.exe'
if (-not (Test-Path $Exe)) { throw "missing $Exe - build first" }

# Check the PE header: Machine must match -Arch, so an x64 build can never ship labelled arm64.
$bytes = [System.IO.File]::ReadAllBytes($Exe)
$pe = [BitConverter]::ToInt32($bytes, 0x3C)
$machine = [BitConverter]::ToUInt16($bytes, $pe + 4)
$expected = switch ($Arch) { 'x64' { 0x8664 } 'arm64' { 0xAA64 } }
if ($machine -ne $expected) { throw ("kalem.exe machine type 0x{0:X} does not match -Arch {1}" -f $machine, $Arch) }

& (Join-Path $PSScriptRoot 'sign.ps1') $Exe

# ---- portable zip ------------------------------------------------------------------------------
$Stage = Join-Path $TargetDir "windows-package\$Name"
Remove-Item -Recurse -Force (Split-Path $Stage) -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $Stage | Out-Null
Copy-Item $Exe $Stage
Copy-Item -Recurse $LangDir (Join-Path $Stage 'languages')
Copy-Item (Join-Path $Root 'README.md'), (Join-Path $Root 'LICENSE-MIT'), (Join-Path $Root 'LICENSE-APACHE') $Stage
$Zip = Join-Path $Dist "$Name-portable.zip"
Remove-Item -Force $Zip -ErrorAction SilentlyContinue
Compress-Archive -Path $Stage -DestinationPath $Zip

# ---- MSI ---------------------------------------------------------------------------------------
$Msi = Join-Path $Dist "$Name.msi"
Remove-Item -Force $Msi -ErrorAction SilentlyContinue
$wixArch = if ($Arch -eq 'x64') { 'x64' } else { 'arm64' }
Invoke-Native 'wix build' {
  wix build (Join-Path $PSScriptRoot 'kalem.wxs') -arch $wixArch `
    -d "Version=$MsiVersion" -d "BinDir=$Bin" -d "LangDir=$LangDir" -d "RootDir=$Root" -o $Msi
}
& (Join-Path $PSScriptRoot 'sign.ps1') $Msi

foreach ($f in @($Zip, $Msi)) {
  $hash = (Get-FileHash $f -Algorithm SHA256).Hash.ToLower()
  "$hash  $(Split-Path $f -Leaf)" | Out-File "$f.sha256" -Encoding ascii
  Write-Output "created $f"
}
