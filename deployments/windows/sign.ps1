<#
.SYNOPSIS
  Authenticode-sign files with signtool, using whichever signing material is configured.

.DESCRIPTION
  Tries, in order:
    1. A certificate file:   WINDOWS_CERTIFICATE (base64 .pfx), WINDOWS_CERTIFICATE_PASSWORD
    2. Azure Trusted Signing: AZURE_TENANT_ID, AZURE_CLIENT_ID, AZURE_CLIENT_SECRET,
                              AZURE_SIGNING_ENDPOINT, AZURE_SIGNING_ACCOUNT, AZURE_CERT_PROFILE
       (needs the Trusted Signing dlib; set AZURE_CODESIGN_DLIB to its path)
  With neither, it warns and leaves the files unsigned (exit code 0), so test builds still
  produce installers. This is the one place to adapt when the signing setup changes.
  Optional: WINDOWS_TIMESTAMP_URL, SIGNTOOL (path to signtool.exe).

.EXAMPLE
  pwsh deployments/windows/sign.ps1 dist/kalem.exe dist/kalem.msi
#>
param([Parameter(Mandatory = $true, ValueFromRemainingArguments = $true)] [string[]] $Files)
$ErrorActionPreference = 'Stop'

function Write-Warn([string] $Message) {
  if ($env:GITHUB_ACTIONS) { Write-Output "::warning::$Message" } else { Write-Warning $Message }
}

function Find-SignTool {
  if ($env:SIGNTOOL) { return $env:SIGNTOOL }
  $onPath = Get-Command signtool.exe -ErrorAction SilentlyContinue
  if ($onPath) { return $onPath.Source }
  $kits = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
  $found = Get-ChildItem -Path $kits -Recurse -Filter signtool.exe -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -match '\\x64\\signtool\.exe$' } |
    Sort-Object FullName -Descending | Select-Object -First 1
  if (-not $found) { throw 'signtool.exe not found (install the Windows SDK)' }
  return $found.FullName
}

$Files = $Files | ForEach-Object { (Resolve-Path $_).Path }
$haveCert = [bool]$env:WINDOWS_CERTIFICATE
$azureVars = 'AZURE_TENANT_ID', 'AZURE_CLIENT_ID', 'AZURE_CLIENT_SECRET', 'AZURE_SIGNING_ENDPOINT', 'AZURE_SIGNING_ACCOUNT', 'AZURE_CERT_PROFILE', 'AZURE_CODESIGN_DLIB'
$haveAzure = -not ($azureVars | Where-Object { -not [Environment]::GetEnvironmentVariable($_) })

if (-not $haveCert -and -not $haveAzure) {
  Write-Warn "Windows signing secrets not set: leaving unsigned: $($Files -join ', ')"
  exit 0
}

$signtool = Find-SignTool
$timestamp = if ($env:WINDOWS_TIMESTAMP_URL) { $env:WINDOWS_TIMESTAMP_URL } else { 'http://timestamp.acs.microsoft.com' }
$common = @('sign', '/v', '/fd', 'SHA256', '/td', 'SHA256', '/tr', $timestamp, '/d', 'Kalem IDE')

if ($haveCert) {
  $pfx = Join-Path ([IO.Path]::GetTempPath()) "kalem-sign-$PID.pfx"
  try {
    [IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:WINDOWS_CERTIFICATE))
    & $signtool @common '/f' $pfx '/p' $env:WINDOWS_CERTIFICATE_PASSWORD @Files
    if ($LASTEXITCODE -ne 0) { throw "signtool failed with exit code $LASTEXITCODE" }
  } finally {
    Remove-Item -Force $pfx -ErrorAction SilentlyContinue
  }
} else {
  $meta = Join-Path ([IO.Path]::GetTempPath()) "kalem-sign-$PID.json"
  try {
    @{ Endpoint = $env:AZURE_SIGNING_ENDPOINT; CodeSigningAccountName = $env:AZURE_SIGNING_ACCOUNT; CertificateProfileName = $env:AZURE_CERT_PROFILE } |
      ConvertTo-Json | Set-Content -Encoding utf8 $meta
    & $signtool @common '/dlib' $env:AZURE_CODESIGN_DLIB '/dmdf' $meta @Files
    if ($LASTEXITCODE -ne 0) { throw "signtool failed with exit code $LASTEXITCODE" }
  } finally {
    Remove-Item -Force $meta -ErrorAction SilentlyContinue
  }
}
Write-Output "signed: $($Files -join ', ')"
