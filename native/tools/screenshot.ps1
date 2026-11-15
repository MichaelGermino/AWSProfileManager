<#
  Launch the 2.0 app on a sandbox data directory, capture its main window to a PNG, then close it.
  Uses PrintWindow(PW_RENDERFULLCONTENT), which captures GPUI windows (Phase 0 finding).

  .\native\tools\screenshot.ps1 -DataDir native\tests\fixtures\datadirs\two-folders -Out .screens\gpui-profiles.png
  Optional: -Settings '{"theme":"light"}' merges into the sandbox settings.json; -Wait seconds.
#>
param(
  [string]$DataDir = "",
  [string]$Out = ".screens\gpui.png",
  [string]$Settings = "",
  [string]$Credentials = "",
  [int]$Wait = 4,
  [string]$Exe = "native\target\fastrel\aws-profile-manager.exe"
)
$ErrorActionPreference = 'Stop'
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class Win {
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hwnd, IntPtr hdc, uint flags);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
}
"@
Add-Type -AssemblyName System.Drawing
[Win]::SetProcessDPIAware() | Out-Null

$sandbox = Join-Path $env:TEMP ("apm-shot-" + [guid]::NewGuid().ToString('N').Substring(0, 8))
$appData = Join-Path $sandbox 'appdata'
New-Item -ItemType Directory -Force $appData | Out-Null
if ($DataDir) { Copy-Item -Recurse -Force (Join-Path $DataDir '*') $appData }
$settingsFile = Join-Path $appData 'settings.json'
$base = @{ setupCompleted = $true; seenTourIds = @('v1.5-features-ga', 'v1.6-features'); lastChangelogVersionSeen = '9.9.9' }
if (Test-Path $settingsFile) { $existing = Get-Content $settingsFile -Raw | ConvertFrom-Json; foreach ($p in $existing.PSObject.Properties) { $base[$p.Name] = $p.Value } }
if ($Settings) { $extra = $Settings | ConvertFrom-Json; foreach ($p in $extra.PSObject.Properties) { $base[$p.Name] = $p.Value } }
$utf8 = New-Object System.Text.UTF8Encoding($false)  # no BOM: JSON.parse / serde reject one
[IO.File]::WriteAllText($settingsFile, ($base | ConvertTo-Json -Depth 10), $utf8)
$creds = Join-Path $sandbox 'aws\credentials'
New-Item -ItemType Directory -Force (Split-Path $creds) | Out-Null
if ($Credentials) { [IO.File]::WriteAllText($creds, $Credentials, $utf8) }

$env:APM_TEST_MODE = '1'
$env:APM_APPDATA_DIR = $appData
$env:APM_AWS_CREDENTIALS_FILE = $creds
$env:APM_KEYCHAIN_SERVICE = 'AWSProfileManager-shot'
# Keep every real host off the network (same idea as the parity suite's blackhole).
$env:APM_HOST_MAP = '{"api.github.com":"https://127.0.0.1:9","docs.aws.amazon.com":"https://127.0.0.1:9","sts.us-west-2.amazonaws.com":"https://127.0.0.1:9"}'

$proc = Start-Process -FilePath $Exe -PassThru
try {
  Start-Sleep -Seconds $Wait
  $proc.Refresh()
  $hwnd = $proc.MainWindowHandle
  if ($hwnd -eq [IntPtr]::Zero) { throw "no main window" }
  $r = New-Object Win+RECT
  [Win]::GetWindowRect($hwnd, [ref]$r) | Out-Null
  $w = $r.Right - $r.Left; $h = $r.Bottom - $r.Top
  $bmp = New-Object System.Drawing.Bitmap $w, $h
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $hdc = $g.GetHdc()
  [Win]::PrintWindow($hwnd, $hdc, 2) | Out-Null
  $g.ReleaseHdc($hdc); $g.Dispose()
  New-Item -ItemType Directory -Force (Split-Path $Out) | Out-Null
  $bmp.Save((Resolve-Path -LiteralPath (Split-Path $Out)).Path + '\' + (Split-Path $Out -Leaf), [System.Drawing.Imaging.ImageFormat]::Png)
  $bmp.Dispose()
  Write-Output "saved $Out ($w x $h)"
} finally {
  Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
  Remove-Item -Recurse -Force $sandbox -ErrorAction SilentlyContinue
}
