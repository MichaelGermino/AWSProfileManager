<#
  Launch the 2.0 app on a sandbox data directory, capture its main window to a PNG, then close it.
  Uses PrintWindow(PW_RENDERFULLCONTENT), which captures GPUI windows (Phase 0 finding).

  .\native\tools\screenshot.ps1 -DataDir native\tests\fixtures\datadirs\two-folders -Out .screens\gpui-profiles.png
  Optional: -Settings '{"theme":"light"}' merges into the sandbox settings.json; -Wait seconds.
  -Secrets '{"__default__":"plain"}' seeds keychain entries (service AWSProfileManager-shot), removed after.
  -Click 'Refresh Dev','Sign in' invokes controls by accessible name (UI Automation), in order.
  -Type 'Username=me@corp' sets a text field by name before the clicks; 'Username=me@corp' inside
  -Click types at that point in the sequence.
#>
param(
  [string]$DataDir = "",
  [string]$Out = ".screens\gpui.png",
  [string]$Settings = "",
  [string]$Credentials = "",
  [int]$Wait = 4,
  [string]$Secrets = "",
  [string[]]$Click = @(),
  [string[]]$Type = @(),
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
  [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr ctx);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int x, int y, uint d, IntPtr e);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, IntPtr extra);
  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] public struct CREDENTIAL {
    public int Flags; public int Type; public string TargetName; public string Comment; public long LastWritten;
    public int CredentialBlobSize; public IntPtr CredentialBlob; public int Persist; public int AttributeCount;
    public IntPtr Attributes; public string TargetAlias; public string UserName; }
  [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)] public static extern bool CredWrite(ref CREDENTIAL c, int flags);
  [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)] public static extern bool CredDelete(string target, int type, int flags);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
}
"@
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Windows.Forms
$service = 'AWSProfileManager-shot'
$seeded = @()
function Set-Secret([string]$account, [string]$value) {
  # Same layout as the keyring crate: generic credential "<service>/<account>", UTF-8 blob.
  $bytes = [Text.Encoding]::UTF8.GetBytes($value)
  $ptr = [Runtime.InteropServices.Marshal]::AllocHGlobal($bytes.Length)
  [Runtime.InteropServices.Marshal]::Copy($bytes, 0, $ptr, $bytes.Length)
  $c = New-Object Win+CREDENTIAL
  $c.Type = 1; $c.TargetName = "$service/$account"; $c.UserName = $account; $c.Persist = 2
  $c.CredentialBlob = $ptr; $c.CredentialBlobSize = $bytes.Length
  if (-not [Win]::CredWrite([ref]$c, 0)) { throw "CredWrite failed" }
  [Runtime.InteropServices.Marshal]::FreeHGlobal($ptr)
  $script:seeded += "$service/$account"
}
function Set-Field($root, [string]$name, [string]$value) {
  # The visible label carries the same name as the field; take the element that accepts a value.
  $cond = New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::NameProperty, $name)
  for ($i = 0; $i -lt 20; $i++) {
    foreach ($el in $root.FindAll([Windows.Automation.TreeScope]::Descendants, $cond)) {
      $vp = $null
      if ($el.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern, [ref]$vp)) { $vp.SetValue($value); return }
    }
    # Password fields expose no value pattern: focus and type.
    foreach ($el in $root.FindAll([Windows.Automation.TreeScope]::Descendants, $cond)) {
      if ($el.Current.IsKeyboardFocusable) {
        [Win]::SetForegroundWindow($hwnd) | Out-Null
        $el.SetFocus()
        Start-Sleep -Milliseconds 150
        [Windows.Forms.SendKeys]::SendWait(($value -replace '([+^%~(){}\[\]])', '{$1}'))
        return
      }
    }
    Start-Sleep -Milliseconds 250
  }
  throw "no editable field named '$name'"
}
function Find-ByName($root, [string]$name) {
  $cond = New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::NameProperty, $name)
  for ($i = 0; $i -lt 20; $i++) {
    $el = $root.FindFirst([Windows.Automation.TreeScope]::Descendants, $cond)
    if ($el) { return $el }
    Start-Sleep -Milliseconds 250
  }
  $all = $root.FindAll([Windows.Automation.TreeScope]::Descendants, [Windows.Automation.Condition]::TrueCondition)
  $names = ($all | ForEach-Object { "$($_.Current.ControlType.ProgrammaticName -replace 'ControlType.','')='$($_.Current.Name)'" }) -join ', '
  throw "no element named '$name'. Present: $names"
}
# Per-monitor v2, so window, UIA and cursor coordinates agree on mixed-DPI desktops.
if (-not [Win]::SetProcessDpiAwarenessContext([IntPtr](-4))) { [Win]::SetProcessDPIAware() | Out-Null }

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
$env:APM_KEYCHAIN_SERVICE = $service
# Keep every real host off the network (same idea as the parity suite's blackhole).
$env:APM_HOST_MAP = '{"api.github.com":"https://127.0.0.1:9","docs.aws.amazon.com":"https://127.0.0.1:9","sts.us-west-2.amazonaws.com":"https://127.0.0.1:9"}'

if ($Secrets) { $sec = $Secrets | ConvertFrom-Json; foreach ($p in $sec.PSObject.Properties) { Set-Secret $p.Name ([string]$p.Value) } }

$proc = Start-Process -FilePath $Exe -PassThru
try {
  Start-Sleep -Seconds $Wait
  $proc.Refresh()
  $hwnd = $proc.MainWindowHandle
  if ($hwnd -eq [IntPtr]::Zero) { throw "no main window" }
  if ($Click.Count -gt 0 -or $Type.Count -gt 0) {
    $root = [Windows.Automation.AutomationElement]::FromHandle($hwnd)
    foreach ($t in $Type) {
      $name, $value = $t -split '=', 2
      Set-Field $root $name $value
    }
    foreach ($c in $Click) {
      if ($c -match '=') {
        # "Name=value" in the click list types into that field at this point in the sequence.
        $name, $value = $c -split '=', 2
        Set-Field $root $name $value
        continue
      }
      $el = Find-ByName $root $c
      $inv = $null
      if ($el.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern, [ref]$inv)) { $inv.Invoke(); if ($env:APM_SHOT_DEBUG) { Write-Host "invoke $c" } }
      else {
        # No Invoke pattern (for example a menu trigger): click the middle of the element.
        $b = $el.Current.BoundingRectangle
        # A tapped Alt lets a background process take the foreground (focus-stealing rule).
        [Win]::keybd_event(0x12, 0, 0, [IntPtr]::Zero); [Win]::keybd_event(0x12, 0, 2, [IntPtr]::Zero)
        [Win]::SetForegroundWindow($hwnd) | Out-Null
        Start-Sleep -Milliseconds 150
        [Win]::SetCursorPos([int]($b.X + $b.Width / 2), [int]($b.Y + $b.Height / 2)) | Out-Null
        Start-Sleep -Milliseconds 250
        [Win]::mouse_event(2, 0, 0, 0, [IntPtr]::Zero); Start-Sleep -Milliseconds 60; [Win]::mouse_event(4, 0, 0, 0, [IntPtr]::Zero)
      }
      Start-Sleep -Milliseconds 700
    }
  }
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
  foreach ($t in $seeded) { [Win]::CredDelete($t, 1, 0) | Out-Null }
  Remove-Item -Recurse -Force $sandbox -ErrorAction SilentlyContinue
}
