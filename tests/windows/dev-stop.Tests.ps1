$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot '../../scripts/windows-dev-stop-probe.ps1')
Add-Type -Path (Join-Path $PSScriptRoot '../../scripts/windows-dev-rebuild-process.cs')
$script:passed = 0
function Assert($condition, [string] $message) { if (-not $condition) { throw $message } }
function Probe([bool[]] $exits) {
  $probe = [pscustomobject]@{ Exits = [Collections.Generic.Queue[bool]]::new(); Waits = @(); Disposed = $false }
  foreach ($exited in $exits) { $probe.Exits.Enqueue($exited) }
  $probe | Add-Member ScriptMethod WaitForExit {
    param($milliseconds)
    $this.Waits += $milliseconds
    return $this.Exits.Dequeue()
  }
  $probe | Add-Member ScriptMethod Dispose { $this.Disposed = $true }
  return $probe
}
function Pass([string] $name) { $script:passed++; Write-Output "ok $script:passed - $name" }
$row = [pscustomobject]@{ pid = 42 }
$absent = Get-DevCleanupProcess $row { param($id) Assert ($id -eq 42) 'wrong cleanup PID'; $null } { throw 'must not inspect absent process' }
Assert ($null -eq $absent) 'absent process must succeed'
Pass 'no pinned handle means already exited'

$probe = Probe @($true)
$result = Get-DevCleanupProcess $row { $probe } { throw 'must not inspect exited process' }
Assert ($null -eq $result -and $probe.Disposed) 'exited handle was retained'
Pass 'exit before image open bypasses image inspection'

$probe = Probe @($false, $true)
$result = Get-DevCleanupProcess $row { $probe } { throw [ComponentModel.Win32Exception]::new(5, 'Open DEV image') }
Assert ($null -eq $result -and $probe.Disposed) 'exit during image open must succeed'
Assert (($probe.Waits -join ',') -eq '0,10000') 'must await the same pinned handle'
Pass 'image access error followed by pinned exit succeeds'

foreach ($failure in @('access', 'wrong image')) {
  $probe = Probe @($false, $false)
  $caught = $null
  try {
    Get-DevCleanupProcess $row { $probe } {
      if ($failure -eq 'access') { throw [ComponentModel.Win32Exception]::new(5, 'Open DEV image') }
      throw 'DEV instance image or creation identity changed; refusing cleanup'
    } | Out-Null
  } catch { $caught = $_ }
  Assert ($null -ne $caught -and $probe.Disposed) "live $failure must refuse and dispose"
  Assert (($probe.Waits -join ',') -eq '0,10000') 'live refusal must check pinned exit'
  if ($failure -eq 'access') { Assert ($caught.Exception.Message -match 'Open DEV image') 'lost image access failure' }
  else { Assert ($caught.Exception.Message -match 'identity changed') 'lost identity refusal' }
  Pass "live pinned process with $failure refuses cleanup"
}

$probe = Probe @($false)
$script:validated = $false
$result = Get-DevCleanupProcess $row { $probe } { param($process, $expected) $script:validated = ($process -eq $probe -and $expected.pid -eq 42) }
Assert ($result -eq $probe -and $script:validated -and -not $probe.Disposed) 'valid live process lost pinned ownership'
$result.Dispose()
Pass 'validated live process retains the exact handle for stop'
Write-Output "# pass $script:passed"
Write-Output '# fail 0'
