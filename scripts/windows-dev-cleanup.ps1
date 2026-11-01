# Caller: build.sh gateway-fake|gateway-sleep|gateway-status|gateway-cancel|gateway-policy-probe; scripts check-windows-dev-cleanup-selection.ps1
param(
  [Parameter(Mandatory = $true)][string] $Root,
  [Parameter(Mandatory = $true)][string] $TargetDebug,
  [Parameter(Mandatory = $true)][string] $DataDir,
  [Parameter(Mandatory = $true)][string] $Identifier,
  [Parameter(Mandatory = $false)][string] $Instance = "",
  [Parameter(Mandatory = $true)][int] $VitePort,
  [Parameter(Mandatory = $true)][int] $ApiPort,
  [Parameter(Mandatory = $true)][int] $CdpPort,
  [Parameter(Mandatory = $false)][string[]] $OwnedProcessRecords = @(),
  [switch] $OwnedOnly,
  [switch] $KeepVite,
  [string] $FixturePath,
  [switch] $PlanOnly
)

$ErrorActionPreference = "Stop"
if (@($ApiPort, $VitePort, $CdpPort) -contains 7685) { throw 'Refusing production port 7685' }
function Remove-VerbatimPrefix([string] $path) {
  # Win32_Process can report image paths with the \\?\ prefix, which breaks prefix matching.
  if ($path.StartsWith('\\?\')) { return $path.Substring(4) }
  return $path
}

$rootPath = [IO.Path]::GetFullPath($Root)
$targetDebugPath = [IO.Path]::GetFullPath($TargetDebug)
$dataDirPath = [IO.Path]::GetFullPath($DataDir)
$nodeModules = Join-Path $rootPath "node_modules"
$runtimeImages = @('domios-app.exe', 'domios-dev-app.exe', 'domios-server.exe', 'codeterm.exe',
  'domios-server/domios-server.exe', 'cli/codeterm.exe', 'conpty/x64/OpenConsole.exe', 'conpty/arm64/OpenConsole.exe') |
  ForEach-Object { [IO.Path]::GetFullPath((Join-Path $targetDebugPath $_)) }

if ($FixturePath) {
  if (-not $PlanOnly) { throw 'Process fixtures require PlanOnly; refusing live cleanup' }
  $fixture = Get-Content -Raw -LiteralPath $FixturePath | ConvertFrom-Json
  $all = @($fixture.processes)
  $listeners = @($fixture.listeners)
  $selfPid = [int]$fixture.self_pid
} else {
  $all = @(Get-CimInstance Win32_Process)
  $listeners = @()
  foreach ($line in @(netstat -ano | Select-String "LISTENING")) {
    if ($line.ToString() -match '^\s*\S+\s+\S+:(\d+)\s+\S+\s+LISTENING\s+(\d+)\s*$') {
      $listeners += [pscustomobject]@{ port = [int]$matches[1]; process_id = [int]$matches[2] }
    }
  }
  $selfPid = $PID
}

$childrenByParent = @{}
$parentByPid = @{}
$portsByPid = @{}
foreach ($listener in $listeners) {
  $procId = [int]$listener.process_id
  if (-not $portsByPid.ContainsKey($procId)) {
    $portsByPid[$procId] = New-Object System.Collections.Generic.List[int]
  }
  $portsByPid[$procId].Add([int]$listener.port)
}
foreach ($proc in $all) {
  $parent = [int]$proc.ParentProcessId
  $parentByPid[[int]$proc.ProcessId] = $parent
  if (-not $childrenByParent.ContainsKey($parent)) {
    $childrenByParent[$parent] = New-Object System.Collections.Generic.List[object]
  }
  $childrenByParent[$parent].Add($proc)
}

$selfAncestry = New-Object System.Collections.Generic.HashSet[int]
$cursorPid = $selfPid
while ($cursorPid -gt 0 -and $selfAncestry.Add($cursorPid)) {
  if (-not $parentByPid.ContainsKey($cursorPid)) {
    break
  }
  $cursorPid = [int]$parentByPid[$cursorPid]
}

$victimRoots = New-Object System.Collections.Generic.HashSet[int]
$pinned = @{}
$identities = @{}
$skipped = New-Object System.Collections.Generic.List[string]
if (-not $FixturePath) {
  Add-Type -Path (Join-Path $PSScriptRoot 'windows-dev-rebuild-process.cs')
}
function Get-ProcessIdentity($row) {
  $procId = [int]$row.ProcessId
  if ($identities.ContainsKey($procId)) { return $identities[$procId] }
  if ($FixturePath) {
    $identity = @{ pid = $procId; created = [string]$row.created; executable = [string]$row.ExecutablePath }
  } else {
    $process = [DevRebuildProcess]::Open($procId)
    if ($null -eq $process) { throw 'process already exited' }
    try {
      $identity = @{ pid = $procId; created = $process.Created; executable = $process.Executable }
      if (-not (Remove-VerbatimPrefix $identity.executable).Equals((Remove-VerbatimPrefix ([string]$row.ExecutablePath)), [StringComparison]::OrdinalIgnoreCase) -or
          [decimal]::Truncate([decimal]$identity.created / 10) -ne [decimal]::Truncate(([datetime]$row.CreationDate).ToUniversalTime().ToFileTimeUtc() / [decimal]10)) {
        throw 'process changed since snapshot'
      }
      $pinned[$procId] = $process
    } catch { $process.Dispose(); throw }
  }
  if (-not $identity.executable -or $identity.created -notmatch '^\d+$') { throw 'process identity unavailable' }
  $identities[$procId] = $identity
  return $identity
}
function Skip-Process([string]$reason) {
  $skipped.Add($reason)
  Write-Warning "DEV cleanup skipped: $reason"
}
foreach ($recordPath in $OwnedProcessRecords) {
  try {
    $record = Get-Content -Raw -LiteralPath $recordPath | ConvertFrom-Json
    $procId = [int]$record.pid
    if ($procId -le 0 -or $selfAncestry.Contains($procId)) { throw 'invalid or ancestor PID' }
    $row = $all | Where-Object { [int]$_.ProcessId -eq $procId } | Select-Object -First 1
    if (-not $row) { throw 'recorded process exited' }
    $identity = Get-ProcessIdentity $row
    if ($identity.created -cne [string]$record.created -or -not $record.executable -or
        -not (Remove-VerbatimPrefix $identity.executable).Equals((Remove-VerbatimPrefix ([string]$record.executable)), [StringComparison]::OrdinalIgnoreCase)) {
      throw 'recorded PID identity mismatch'
    }
    [void]$victimRoots.Add($procId)
  } catch {
    Skip-Process "identity record '$recordPath': $($_.Exception.Message)"
  }
}
if (-not $OwnedOnly) {
  foreach ($proc in $all) {
    $procId = [int]$proc.ProcessId
    if ($selfAncestry.Contains($procId)) {
      continue
    }
    $cmd = [string]$proc.CommandLine
    $exe = Remove-VerbatimPrefix ([string]$proc.ExecutablePath)
    $name = [string]$proc.Name
    $procPorts = if ($portsByPid.ContainsKey($procId)) { @($portsByPid[$procId]) } else { @() }
    $rawTarget = ($exe -and $exe.StartsWith($targetDebugPath, [StringComparison]::OrdinalIgnoreCase)) -or
      ($cmd -and $cmd.IndexOf($targetDebugPath, [StringComparison]::OrdinalIgnoreCase) -ge 0)
    if ($Instance -eq '') { $rawTarget = $exe -and $exe -in $runtimeImages }
    $instanceConptyRoot = Join-Path $dataDirPath "target\debug\conpty"
    $instanceConptyPath = $name.Equals("OpenConsole.exe", [StringComparison]::OrdinalIgnoreCase) -and
      (($exe -and $exe.StartsWith($instanceConptyRoot, [StringComparison]::OrdinalIgnoreCase)) -or
        ($cmd -and $cmd.IndexOf($instanceConptyRoot, [StringComparison]::OrdinalIgnoreCase) -ge 0))
    $parentGone = -not $parentByPid.ContainsKey([int]$proc.ParentProcessId)
    $parentIsThisDaemon = @($listeners | Where-Object {
        [int]$_.port -eq $ApiPort -and [int]$_.process_id -eq [int]$proc.ParentProcessId
      }).Count -gt 0
    $matchesInstanceConpty = $instanceConptyPath -and ($parentGone -or $parentIsThisDaemon)
    $ownsApi = $procPorts -contains $ApiPort
    $matchesTarget = $rawTarget -and ($ownsApi -or ($Instance -eq "" -and $procPorts.Count -eq 0))
    $matchesWebView = $cmd -and
      $cmd.IndexOf("$Identifier\EBWebView", [StringComparison]::OrdinalIgnoreCase) -ge 0
    $isRepoScriptHost = @("node.exe", "npm.exe", "pnpm.exe", "cmd.exe", "sh.exe", "bash.exe") -contains $name.ToLowerInvariant()
    $matchesVite = -not $KeepVite -and $isRepoScriptHost -and $cmd -and
      $cmd.IndexOf($nodeModules, [StringComparison]::OrdinalIgnoreCase) -ge 0 -and
      ($cmd -match 'vite[\\/]bin[\\/]vite\.js|@tauri-apps[\\/]cli[\\/]tauri\.js') -and
      ($cmd.IndexOf(":$VitePort", [StringComparison]::OrdinalIgnoreCase) -ge 0 -or
        $cmd.IndexOf("--port $VitePort", [StringComparison]::OrdinalIgnoreCase) -ge 0 -or
        $cmd.IndexOf("CODETERM_VITE_PORT=$VitePort", [StringComparison]::OrdinalIgnoreCase) -ge 0)
    $matchesCdp = $cmd -and
      $cmd.IndexOf("--remote-debugging-port=$CdpPort", [StringComparison]::OrdinalIgnoreCase) -ge 0 -and
      $name.Equals('msedgewebview2.exe', [StringComparison]::OrdinalIgnoreCase) -and
      $cmd.IndexOf("$Identifier\EBWebView", [StringComparison]::OrdinalIgnoreCase) -ge 0

    if ($matchesTarget -or $matchesInstanceConpty -or $matchesWebView -or $matchesVite -or $matchesCdp) {
      [void]$victimRoots.Add($procId)
    }
  }
}

$conptyRoot = (Join-Path $targetDebugPath "conpty")
$conptyPrefix = $conptyRoot.TrimEnd('\', '/') + '\'
if (-not $OwnedOnly) {
  $daemonPids = New-Object System.Collections.Generic.HashSet[int]
  foreach ($listener in $listeners) {
    if ([int]$listener.port -eq $ApiPort) {
      [void]$daemonPids.Add([int]$listener.process_id)
    }
  }
  foreach ($proc in $all) {
    if (-not ([string]$proc.Name).Equals("OpenConsole.exe", [StringComparison]::OrdinalIgnoreCase)) {
      continue
    }
    $procId = [int]$proc.ProcessId
    if ($selfAncestry.Contains($procId)) {
      continue
    }
    $parentId = [int]$proc.ParentProcessId
    $imagePath = Remove-VerbatimPrefix ([string]$proc.ExecutablePath)
    $underThisTarget = $imagePath.StartsWith($conptyPrefix, [StringComparison]::OrdinalIgnoreCase)
    $parentGone = -not $parentByPid.ContainsKey($parentId)
    $parentIsThisDaemon = $daemonPids.Contains($parentId)
    if ($underThisTarget -and ($parentGone -or $parentIsThisDaemon)) {
      [void]$victimRoots.Add($procId)
    }
  }
}

$selected = New-Object System.Collections.Generic.HashSet[int]
function Add-ProcessTree {
  param([int] $ProcessId, [long] $ParentCreated = 0)
  if ($ProcessId -le 0 -or $selfAncestry.Contains($ProcessId) -or $selected.Contains($ProcessId)) {
    return
  }
  if ($portsByPid.ContainsKey($ProcessId) -and $portsByPid[$ProcessId].Contains(7685)) {
    Skip-Process "PID ${ProcessId}: production listener"
    return
  }
  try {
    $row = $all | Where-Object { [int]$_.ProcessId -eq $ProcessId } | Select-Object -First 1
    if (-not $row) { throw 'process exited' }
    $identity = Get-ProcessIdentity $row
    if ([long]$identity.created -lt $ParentCreated) { throw 'child predates parent incarnation' }
  } catch { Skip-Process "PID ${ProcessId}: $($_.Exception.Message)"; return }
  [void]$selected.Add($ProcessId)
  if ($childrenByParent.ContainsKey($ProcessId)) {
    foreach ($child in $childrenByParent[$ProcessId]) {
      Add-ProcessTree -ProcessId ([int]$child.ProcessId) -ParentCreated ([long]$identity.created)
    }
  }
}
foreach ($procId in @($victimRoots)) {
  Add-ProcessTree -ProcessId $procId
}

if ($PlanOnly) {
  foreach ($process in $pinned.Values) { $process.Dispose() }
  [pscustomobject]@{
    instance = $Instance
    identifier = $Identifier
    api_port = $ApiPort
    vite_port = $VitePort
    cdp_port = $CdpPort
    victim_roots = @($victimRoots | Sort-Object)
    selected_processes = @($selected | Sort-Object)
    skipped = @($skipped)
  } | ConvertTo-Json -Depth 4
  exit 0
}

$seen = @{}
function Stop-ProcessTree {
  param([int] $ProcessId)
  if ($ProcessId -le 0 -or $seen.ContainsKey($ProcessId) -or -not $selected.Contains($ProcessId)) {
    return
  }
  $seen[$ProcessId] = $true
  if ($childrenByParent.ContainsKey($ProcessId)) {
    foreach ($child in $childrenByParent[$ProcessId]) {
      Stop-ProcessTree -ProcessId ([int]$child.ProcessId)
    }
  }
  try { [DevRebuildProcess]::Stop($pinned[$ProcessId], 10000) }
  catch { Skip-Process "PID ${ProcessId}: $($_.Exception.Message)" }
}
try {
  foreach ($procId in @($victimRoots)) {
    Stop-ProcessTree -ProcessId $procId
  }
} finally {
  foreach ($process in $pinned.Values) { $process.Dispose() }
}
