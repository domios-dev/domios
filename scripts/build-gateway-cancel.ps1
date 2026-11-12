param(
  [Parameter(Mandatory = $true)][int] $OwnerPid,
  [Parameter(Mandatory = $true)][long] $OwnerStartToken,
  [Parameter(Mandatory = $true)][int] $OwnerMsysPid,
  [Parameter(Mandatory = $true)][string] $MsysPs
)
$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
public static class GatewayCancelNative {
    [DllImport("ntdll.dll")] static extern int NtSuspendProcess(IntPtr process);
    [DllImport("ntdll.dll")] static extern int NtResumeProcess(IntPtr process);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool TerminateProcess(IntPtr process, uint code);
    public static void Suspend(IntPtr process) {
        int status = NtSuspendProcess(process);
        if (status != 0) throw new InvalidOperationException("Suspend gateway process failed: " + status);
    }
    public static void Resume(IntPtr process) { NtResumeProcess(process); }
    public static void Kill(IntPtr process) {
        if (!TerminateProcess(process, 130)) throw new Win32Exception(Marshal.GetLastWin32Error());
    }
}
'@

$clock = [Diagnostics.Stopwatch]::StartNew()
$processes = New-Object 'System.Collections.Generic.List[object]'
$pinnedIds = New-Object 'System.Collections.Generic.HashSet[int]'
function Remaining {
  $remaining = 20000 - $clock.ElapsedMilliseconds
  if ($remaining -le 0) { throw 'Gateway tree cancellation deadline exceeded' }
  return [int]$remaining
}
function Pin([int] $processId) {
  $proc = $null
  try {
    $proc = [Diagnostics.Process]::GetProcessById($processId)
    $null = $proc.Handle
    if ($proc.HasExited) { $proc.Dispose(); return $null }
    return [pscustomobject]@{ process = $proc; created = $proc.StartTime.ToUniversalTime().Ticks; suspended = $false }
  } catch [ArgumentException] {
    if ($proc) { $proc.Dispose() }
    return $null
  } catch {
    if ($proc) { $proc.Dispose() }
    throw
  }
}

try {
  $root = Pin $OwnerPid
  if (-not $root) { throw 'Gateway owner exited before cancellation' }
  $processes.Add($root)
  [void]$pinnedIds.Add($OwnerPid)
  if ($root.created -ne $OwnerStartToken) { throw 'Gateway owner creation identity changed' }
  for ($index = 0; $index -lt $processes.Count; $index++) {
    $null = Remaining
    $parent = $processes[$index]
    if ($parent.process.HasExited) { continue }
    [GatewayCancelNative]::Suspend($parent.process.Handle)
    $parent.suspended = $true
    $msys = @(& $MsysPs -e | ForEach-Object {
      if ($_ -match '^\s*(\d+)\s+(\d+)\s+\d+\s+(\d+)\s') {
        [pscustomobject]@{ pid = [int]$matches[1]; parent = [int]$matches[2]; native = [int]$matches[3] }
      }
    })
    if ($LASTEXITCODE -ne 0) { throw 'MSYS process discovery failed' }
    $msysParent = @($msys | Where-Object { $_.native -eq $parent.process.Id })
    $msysChildren = @($msys | Where-Object { $_.parent -in $msysParent.pid } | ForEach-Object { $_.native })
    $rows = @(Get-CimInstance Win32_Process | Where-Object {
      $_.ParentProcessId -eq $parent.process.Id -or $_.ProcessId -in $msysChildren
    })
    foreach ($row in $rows) {
      $null = Remaining
      if ($pinnedIds.Contains([int]$row.ProcessId)) { continue }
      if (-not $row.CreationDate -or $row.CreationDate.ToUniversalTime().Ticks -lt ($parent.created - ($parent.created % 10))) { continue }
      $child = Pin ([int]$row.ProcessId)
      if (-not $child) { continue }
      $observed = $row.CreationDate.ToUniversalTime().Ticks
      if (($child.created - ($child.created % 10)) -ne $observed -or $child.created -lt $parent.created) {
        $child.process.Dispose()
        throw 'Gateway descendant creation identity changed'
      }
      $processes.Add($child)
      [void]$pinnedIds.Add([int]$row.ProcessId)
    }
  }
  for ($index = $processes.Count - 1; $index -ge 0; $index--) {
    $entry = $processes[$index]
    $null = Remaining
    if ($index -eq 0 -and -not $entry.process.HasExited) {
      & (Join-Path (Split-Path $MsysPs) 'kill.exe') -s TERM $OwnerMsysPid
      if ($LASTEXITCODE -ne 0) { throw 'MSYS gateway owner signal failed' }
      [GatewayCancelNative]::Resume($entry.process.Handle)
      $entry.suspended = $false
    } elseif (-not $entry.process.HasExited) { [GatewayCancelNative]::Kill($entry.process.Handle) }
    if (-not $entry.process.WaitForExit((Remaining))) { throw 'Gateway process exit was not confirmed' }
    $entry.suspended = $false
  }
} finally {
  foreach ($entry in $processes) {
    if ($entry.suspended) { [GatewayCancelNative]::Resume($entry.process.Handle) }
    $entry.process.Dispose()
  }
}
