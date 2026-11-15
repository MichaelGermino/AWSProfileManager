<#
  Screenshots with realistic data: copies native\tests\fixtures\datadirs\showcase to a temp folder,
  gives each signed-in profile an expiration relative to now (long session, expiring soon, expired,
  never signed in), writes the matching credentials file, then runs screenshot.ps1 on it.
  Never injects mouse or keyboard input (screenshot.ps1 drives controls through UI Automation only).

  .\native\tools\showcase.ps1 -Out .screens\improved\profiles-modern-dark.png
  .\native\tools\showcase.ps1 -Settings '{"theme":"light","appStyle":"classic","profileListLayout":"cards"}' -Out .screens\x.png
  .\native\tools\showcase.ps1 -Click 'Show details for Data lake' -Out .screens\panel.png
  Other parameters (-Wait, -Secrets, -Type, -Exe) pass through to screenshot.ps1.
#>
param(
  [string]$Out = ".screens\showcase.png",
  [string]$Settings = "",
  [string[]]$Click = @(),
  [string[]]$Type = @(),
  [string]$Secrets = "",
  [int]$Wait = 4,
  [string]$Exe = "native\target\fastrel\aws-profile-manager.exe"
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$fixture = Join-Path $PSScriptRoot '..\tests\fixtures\datadirs\showcase'

# Minutes until expiry per profile id; negative = expired that long ago; absent = never signed in.
$expiresIn = @{
  's-platform-prod'    = 7 * 60 + 20
  's-platform-staging' = 9
  's-billing'          = -95
  's-data-lake'        = 3 * 60 + 5
  's-analytics-dev'    = 52
  's-sandbox'          = 11 * 60 + 40
}

$tmp = Join-Path $env:TEMP ("apm-showcase-" + [guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Force $tmp | Out-Null
try {
  Copy-Item -Recurse -Force (Join-Path $fixture '*') $tmp
  $file = Join-Path $tmp 'profiles.json'
  $data = Get-Content $file -Raw | ConvertFrom-Json
  $now = [DateTime]::UtcNow
  $ini = New-Object System.Text.StringBuilder
  foreach ($p in $data.profiles) {
    if (-not $expiresIn.ContainsKey($p.id)) { continue }
    $exp = $now.AddMinutes($expiresIn[$p.id]).AddSeconds(30).ToString("yyyy-MM-ddTHH:mm:ss.fffZ")
    $p | Add-Member -NotePropertyName expiration -NotePropertyValue $exp -Force
    $region = if ($p.ssoRegion) { $p.ssoRegion } else { 'us-west-2' }
    [void]$ini.AppendLine("[$($p.credentialProfileName)]")
    [void]$ini.AppendLine("aws_access_key_id = ASIASHOWCASE$($p.id.Length)EXAMPLE")
    [void]$ini.AppendLine("aws_secret_access_key = showcase/secret/not-real")
    [void]$ini.AppendLine("aws_session_token = showcase-session-token")
    [void]$ini.AppendLine("region = $region")
    [void]$ini.AppendLine("output = json")
    [void]$ini.AppendLine("")
  }
  $utf8 = New-Object System.Text.UTF8Encoding($false)
  [IO.File]::WriteAllText($file, ($data | ConvertTo-Json -Depth 10), $utf8)

  Push-Location $root
  try {
    $shot = @{ DataDir = $tmp; Out = $Out; Credentials = $ini.ToString(); Wait = $Wait; Exe = $Exe }
    if ($Settings) { $shot.Settings = $Settings }
    if ($Secrets) { $shot.Secrets = $Secrets }
    if ($Click.Count -gt 0) { $shot.Click = $Click }
    if ($Type.Count -gt 0) { $shot.Type = $Type }
    & (Join-Path $PSScriptRoot 'screenshot.ps1') @shot
  } finally {
    Pop-Location
  }
} finally {
  Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
