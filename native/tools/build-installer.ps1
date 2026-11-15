<#
  Builds the AWS Profile Manager 2.0 Windows installer (plan §6.5, §8). Never runs it.

    .\native\tools\build-installer.ps1                 # release build + installer
    .\native\tools\build-installer.ps1 -SkipBuild      # reuse native\target\release\aws-profile-manager.exe
    .\native\tools\build-installer.ps1 -Feeds          # also apm-update.json + latest.yml (needs the updater key)

  Output in native\target\dist\:
    AWS-Profile-Manager-Setup-<version>.exe, and with -Feeds: apm-update.json, latest.yml
    (+ <channel>.yml for a pre-release), <installer>.minisig.

  Prerequisites: Rust (MSVC), NSIS 3 (makensis). makensis is looked up in $env:MAKENSIS, PATH,
  Program Files\NSIS, and electron-builder's cache (%LOCALAPPDATA%\electron-builder\Cache\nsis).
  Install NSIS with `winget install NSIS.NSIS` or `choco install nsis` if none is found.
#>
param(
  [string]$Version = "",
  [switch]$SkipBuild,
  [switch]$Feeds,
  [string]$OutDir = ""
)
$ErrorActionPreference = 'Stop'
# Native tools write progress to stderr; Windows PowerShell 5.1 would turn that into errors under 'Stop'.
function Invoke-Native([string]$Exe, [string[]]$Arguments) {
  $ErrorActionPreference = 'Continue'
  & $Exe @Arguments 2>&1 | ForEach-Object { Write-Host "$_" }
  if ($LASTEXITCODE -ne 0) { throw "$Exe failed (exit $LASTEXITCODE)" }
}
$native = Resolve-Path (Join-Path $PSScriptRoot '..')
if (-not $OutDir) { $OutDir = Join-Path $native 'target\dist' }

if (-not $Version) {
  $m = Select-String -Path (Join-Path $native 'Cargo.toml') -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1
  $Version = $m.Matches[0].Groups[1].Value
}
$Version = $Version.TrimStart('v')
$core = ($Version -split '[-+]')[0]
$nums = @($core -split '\.' | ForEach-Object { [int]$_ })
while ($nums.Count -lt 4) { $nums += 0 }
$numeric = ($nums[0..3] -join '.')
Write-Host "Version $Version ($numeric)"

Push-Location $native
try {
  if (-not $SkipBuild) {
    Invoke-Native cargo @('build', '-p', 'apm-app', '--release', '--no-default-features')
  }
  $exe = Join-Path $native 'target\release\aws-profile-manager.exe'
  if (-not (Test-Path $exe)) { throw "$exe is missing (build first, or drop -SkipBuild)" }

  # Release tooling (feeds, notices) in its own target dir, so it never waits on the app build.
  $env:CARGO_TARGET_DIR = Join-Path $native 'target\tooling'
  Invoke-Native cargo @('build', '-p', 'apm-release', '--release')
  Remove-Item Env:CARGO_TARGET_DIR
  $tool = Join-Path $native 'target\tooling\release\apm-release.exe'

  # Staging: exactly what gets installed.
  $stage = Join-Path $OutDir 'stage'
  if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
  New-Item -ItemType Directory -Force (Join-Path $stage 'resources') | Out-Null
  Copy-Item $exe (Join-Path $stage 'AWS Profile Manager.exe')
  $res = Resolve-Path (Join-Path $native '..\resources')
  Copy-Item (Join-Path $res 'icon.ico'), (Join-Path $res 'icon.png') (Join-Path $stage 'resources')
  Copy-Item -Recurse (Join-Path $res 'app-icons') (Join-Path $stage 'resources\app-icons')
  # macOS-only art.
  Get-ChildItem (Join-Path $stage 'resources\app-icons') -Recurse -Filter 'mac.png' | Remove-Item
  Invoke-Native $tool @('notices', '--out', (Join-Path $stage 'THIRD_PARTY_NOTICES.txt'))

  # makensis
  $makensis = $env:MAKENSIS
  if (-not $makensis) { $makensis = (Get-Command makensis -ErrorAction SilentlyContinue).Source }
  foreach ($c in @("${env:ProgramFiles(x86)}\NSIS\makensis.exe", "$env:ProgramFiles\NSIS\makensis.exe")) {
    if (-not $makensis -and (Test-Path $c)) { $makensis = $c }
  }
  if (-not $makensis) {
    $cache = Join-Path $env:LOCALAPPDATA 'electron-builder\Cache\nsis'
    if (Test-Path $cache) {
      $makensis = Get-ChildItem $cache -Recurse -Filter makensis.exe -ErrorAction SilentlyContinue |
        Where-Object { $_.Directory.Name -ne 'Bin' } | Select-Object -First 1 -ExpandProperty FullName
    }
  }
  if (-not $makensis) { throw "makensis not found. Install NSIS 3 (winget install NSIS.NSIS) or set `$env:MAKENSIS." }
  Write-Host "makensis: $makensis"

  $setupName = "AWS-Profile-Manager-Setup-$Version.exe"
  $setup = Join-Path $OutDir $setupName
  $nsi = Join-Path $native 'installer\windows\installer.nsi'
  Invoke-Native $makensis @('/V2', '/INPUTCHARSET', 'UTF8', "/DVERSION=$Version", "/DVERSION_NUMERIC=$numeric", "/DAPP_DIR=$stage", "/DOUT_FILE=$setup", $nsi)

  $size = (Get-Item $setup).Length
  $sha = (Get-FileHash $setup -Algorithm SHA256).Hash.ToLower()
  Write-Host ("Installer: {0} ({1:N1} MB, sha256 {2})" -f $setup, ($size / 1MB), $sha)

  if ($Feeds) {
    Invoke-Native $tool @('feeds', '--installer', $setup, '--version', $Version, '--out', $OutDir)
  }
} finally {
  Pop-Location
}
