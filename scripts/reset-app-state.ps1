<#
.SYNOPSIS
  Reset AWS Profile Manager to a first-run state, for testing setup/onboarding.

.DESCRIPTION
  The app stores data in TWO places, plus the Windows Credential Manager:

    1. %APPDATA%\AWSProfileManager          - profiles.json, settings.json, sso-sessions.json,
                                              ui-prefs.json, rolesCache.json, auth-audit-log.json
                                              (hardcoded name, shared by dev and packaged builds)
    2. Electron userData                    - Partitions/ (persist:sso-* and persist:console-*),
                                              aws-cli-docs-cache/, Local Storage, cookies
                                              %APPDATA%\aws-profile-manager  when run via npm run dev
                                              %APPDATA%\AWS Profile Manager  when packaged
    3. Credential Manager                   - AWSProfileManager/__default__ (IdP username+password)

  Close the app first: it rewrites settings.json on exit and would undo the reset.

  Does NOT touch ~/.aws/credentials by default - that file is shared with the AWS CLI and other
  tools. Use -IncludeAwsCredentials to clear the sections this app manages.

.PARAMETER WhatIf
  Show what would be removed without removing anything.

.PARAMETER KeepCredentials
  Leave the saved IdP username/password in Credential Manager.

.PARAMETER IncludeAwsCredentials
  Also delete ~/.aws/credentials. Destructive and shared with other tools - off by default.

.EXAMPLE
  .\scripts\reset-app-state.ps1 -WhatIf
  .\scripts\reset-app-state.ps1
#>
[CmdletBinding(SupportsShouldProcess = $true)]
param(
    [switch]$KeepCredentials,
    [switch]$IncludeAwsCredentials
)

$ErrorActionPreference = 'Stop'

function Remove-IfPresent([string]$Path, [string]$Label) {
    if (Test-Path -LiteralPath $Path) {
        if ($PSCmdlet.ShouldProcess($Path, "Remove $Label")) {
            Remove-Item -LiteralPath $Path -Recurse -Force
            Write-Host "  removed  $Label" -ForegroundColor Green
            Write-Host "           $Path" -ForegroundColor DarkGray
        } else {
            Write-Host "  WOULD REMOVE  $Label" -ForegroundColor Yellow
            Write-Host "                $Path" -ForegroundColor DarkGray
        }
    } else {
        Write-Host "  absent   $Label" -ForegroundColor DarkGray
    }
}

# Refuse to run while the app is open; it would just rewrite settings.json on exit.
$running = Get-Process -ErrorAction SilentlyContinue |
    Where-Object { $_.ProcessName -in @('AWS Profile Manager', 'electron') }
if ($running) {
    Write-Host "AWS Profile Manager (or an Electron dev instance) is running. Close it first." -ForegroundColor Red
    $running | Select-Object Id, ProcessName | Format-Table | Out-String | Write-Host
    exit 1
}

Write-Host "`nResetting AWS Profile Manager state`n" -ForegroundColor Cyan

Write-Host "App data (profiles, settings, SSO sessions):"
Remove-IfPresent (Join-Path $env:APPDATA 'AWSProfileManager') 'app data'

Write-Host "`nElectron userData (browser sessions, caches):"
Remove-IfPresent (Join-Path $env:APPDATA 'aws-profile-manager') 'userData (dev build)'
Remove-IfPresent (Join-Path $env:APPDATA 'AWS Profile Manager') 'userData (packaged build)'

Write-Host "`nCredential Manager:"
if ($KeepCredentials) {
    Write-Host "  skipped  saved IdP credentials (-KeepCredentials)" -ForegroundColor DarkGray
} else {
    # keytar stores these as generic credentials named AWSProfileManager/<account>.
    $targets = cmdkey /list 2>$null |
        Select-String -Pattern 'AWSProfileManager\S*' -AllMatches |
        ForEach-Object { $_.Matches.Value } |
        Sort-Object -Unique
    if (-not $targets) {
        Write-Host "  absent   no stored credentials" -ForegroundColor DarkGray
    }
    foreach ($t in $targets) {
        if ($PSCmdlet.ShouldProcess($t, 'Delete credential')) {
            cmdkey /delete:$t | Out-Null
            Write-Host "  removed  $t" -ForegroundColor Green
        } else {
            Write-Host "  WOULD REMOVE  $t" -ForegroundColor Yellow
        }
    }
}

Write-Host "`nAWS credentials file:"
$awsCreds = Join-Path $env:USERPROFILE '.aws\credentials'
if ($IncludeAwsCredentials) {
    Remove-IfPresent $awsCreds 'AWS credentials file'
} else {
    Write-Host "  skipped  $awsCreds" -ForegroundColor DarkGray
    Write-Host "           shared with the AWS CLI; pass -IncludeAwsCredentials to delete it" -ForegroundColor DarkGray
}

Write-Host "`nDone. Next launch starts from a clean state.`n" -ForegroundColor Cyan
Write-Host "Note: your browser's AWS multi-session opt-in is a browser cookie and is NOT reset here." -ForegroundColor DarkGray
Write-Host "Clear AWS cookies in the browser if you want to test that path too.`n" -ForegroundColor DarkGray
