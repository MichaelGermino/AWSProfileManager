<#
  Lists what Windows UI Automation (the API Narrator uses) exposes for a window, and optionally
  invokes a named element. Doubles as the proof that a GPUI app can be driven end to end from tests.
  Usage: ./uia-probe.ps1 -Title "GPUI Accessibility Demo" [-Invoke "Reset counter"]
#>
param([string]$Title, [string]$Invoke)
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$root = [System.Windows.Automation.AutomationElement]::RootElement
$cond = New-Object System.Windows.Automation.PropertyCondition ([System.Windows.Automation.AutomationElement]::NameProperty), $Title
$win = $null
for ($i = 0; $i -lt 40 -and -not $win; $i++) { $win = $root.FindFirst('Children', $cond); if (-not $win) { Start-Sleep -Milliseconds 250 } }
if (-not $win) { throw "window '$Title' not found" }

function Walk($el, $depth) {
  $walker = [System.Windows.Automation.TreeWalker]::ControlViewWalker
  $c = $walker.GetFirstChild($el)
  while ($c) {
    $ct = $c.Current.ControlType.ProgrammaticName -replace 'ControlType\.', ''
    $patterns = ($c.GetSupportedPatterns() | ForEach-Object { $_.ProgrammaticName -replace 'PatternIdentifiers\.Pattern|Pattern\.Pattern', '' }) -join ','
    '{0}{1} "{2}" [{3}]' -f ('  ' * $depth), $ct, $c.Current.Name, $patterns
    if ($depth -lt 8) { Walk $c ($depth + 1) }
    $c = $walker.GetNextSibling($c)
  }
}
Walk $win 0

if ($Invoke) {
  $nc = New-Object System.Windows.Automation.PropertyCondition ([System.Windows.Automation.AutomationElement]::NameProperty), $Invoke
  $target = $win.FindFirst('Descendants', $nc)
  if (-not $target) { throw "element '$Invoke' not found" }
  $target.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
  "invoked '$Invoke'"
}
