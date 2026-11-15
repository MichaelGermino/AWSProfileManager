<#
.SYNOPSIS
  Measures memory of an app's whole process tree (Electron helpers or Tauri + WebView2 children).

.EXAMPLE
  ./tests/perf/measure-memory.ps1 -ProcessName "AWS Profile Manager" -Label "1.6.1 profiles idle"
  ./tests/perf/measure-memory.ps1 -RootPid 1234 -Label "tauri tray" -Samples 5 -IntervalSeconds 2 -OutFile perf.jsonl

.NOTES
  Working set = what Task Manager's "Memory" column roughly shows. Private = committed private bytes.
  Both are summed over the root process and every descendant (WebView2 runs as msedgewebview2.exe
  children of the Tauri process). With -Samples > 1 the median of each total is reported.
#>
param(
  [string]$ProcessName,
  [int]$RootPid,
  [string]$Label = '',
  [int]$Samples = 1,
  [int]$IntervalSeconds = 2,
  [string]$OutFile
)

$ErrorActionPreference = 'Stop'

function Get-Tree([int[]]$roots) {
  $all = Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId, Name, CommandLine
  $ids = [System.Collections.Generic.HashSet[int]]::new()
  foreach ($r in $roots) { [void]$ids.Add($r) }
  $added = $true
  while ($added) {
    $added = $false
    foreach ($p in $all) {
      if ($ids.Contains([int]$p.ParentProcessId) -and $ids.Add([int]$p.ProcessId)) { $added = $true }
    }
  }
  $all | Where-Object { $ids.Contains([int]$_.ProcessId) }
}

function Resolve-Roots {
  if ($RootPid) { return @($RootPid) }
  if (-not $ProcessName) { throw 'Pass -ProcessName or -RootPid.' }
  $procs = Get-CimInstance Win32_Process -Filter "Name='$ProcessName.exe'"
  if (-not $procs) { throw "No running process named '$ProcessName.exe'." }
  $pids = @($procs.ProcessId)
  # Roots are the ones whose parent is not part of the same app.
  @($procs | Where-Object { $pids -notcontains $_.ParentProcessId } | ForEach-Object { [int]$_.ProcessId })
}

function Measure-Once {
  $tree = Get-Tree (Resolve-Roots)
  $perf = @{}
  Get-CimInstance Win32_PerfFormattedData_PerfProc_Process | ForEach-Object { $perf[[int]$_.IDProcess] = $_.WorkingSetPrivate }
  $rows = foreach ($p in $tree) {
    $g = Get-Process -Id $p.ProcessId -ErrorAction SilentlyContinue
    if (-not $g) { continue }
    $type = if ($p.CommandLine -match '--type=([\w-]+)') { $matches[1] } else { 'main' }
    [pscustomobject]@{
      pid = [int]$p.ProcessId; name = $p.Name; type = $type
      # Task Manager's default "Memory" column = private working set.
      taskMgrMB = [math]::Round(([double]$perf[[int]$p.ProcessId]) / 1MB, 1)
      wsMB = [math]::Round($g.WorkingSet64 / 1MB, 1)
      privateMB = [math]::Round($g.PrivateMemorySize64 / 1MB, 1)
    }
  }
  [pscustomobject]@{
    processes = @($rows)
    totalTaskMgrMB = [math]::Round(($rows | Measure-Object taskMgrMB -Sum).Sum, 1)
    totalWsMB = [math]::Round(($rows | Measure-Object wsMB -Sum).Sum, 1)
    totalPrivateMB = [math]::Round(($rows | Measure-Object privateMB -Sum).Sum, 1)
  }
}

$runs = @()
for ($i = 0; $i -lt $Samples; $i++) {
  if ($i -gt 0) { Start-Sleep -Seconds $IntervalSeconds }
  $runs += Measure-Once
}

function Median($xs) { $s = $xs | Sort-Object; $s[[math]::Floor(($s.Count - 1) / 2)] }

$last = $runs[-1]
$result = [ordered]@{
  label = $Label
  at = (Get-Date).ToString('o')
  samples = $Samples
  totalTaskMgrMB = Median ($runs | ForEach-Object totalTaskMgrMB)
  totalWsMB = Median ($runs | ForEach-Object totalWsMB)
  totalPrivateMB = Median ($runs | ForEach-Object totalPrivateMB)
  processCount = $last.processes.Count
  processes = $last.processes
}

$last.processes | Sort-Object wsMB -Descending | Format-Table pid, name, type, taskMgrMB, wsMB, privateMB -AutoSize | Out-Host
"{0}: Task Manager {1} MB, working set {2} MB, private {3} MB, {4} processes" -f $Label, $result.totalTaskMgrMB, $result.totalWsMB, $result.totalPrivateMB, $result.processCount | Out-Host

if ($OutFile) { ($result | ConvertTo-Json -Depth 5 -Compress) | Add-Content -Path $OutFile -Encoding utf8 }
