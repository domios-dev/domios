param(
  [Parameter(Mandatory = $true)][string] $Root,
  [Parameter(Mandatory = $true)][string] $TargetDebug,
  [Parameter(Mandatory = $true)][string] $DataDir,
  [Parameter(Mandatory = $true)][string] $Identifier,
  [Parameter(Mandatory = $false)][string] $Instance = "",
  [Parameter(Mandatory = $true)][int] $VitePort,
  [Parameter(Mandatory = $true)][int] $ApiPort,
  [Parameter(Mandatory = $true)][int] $CdpPort,
  [Parameter(Mandatory = $false)][int[]] $OwnedProcessIds = @(),
  [switch] $OwnedOnly,
  [string] $FixturePath,
  [switch] $PlanOnly
)

$ErrorActionPreference = "Stop"
function Remove-VerbatimPrefix([string] $path) {
  # Win32_Process can report image paths with the \\?\ prefix, which breaks prefix matching.
  if ($path.StartsWith('\\?\')) { return $path.Substring(4) }
  return $path
}

$rootPath = [IO.Path]::GetFullPath($Root)
$targetDebugPath = [IO.Path]::GetFullPath($TargetDebug)
$dataDirPath = [IO.Path]::GetFullPath($DataDir)
$nodeModules = Join-Path $rootPath "node_modules"

if ($FixturePath) {
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
foreach ($procId in @($OwnedProcessIds)) {
  if ($procId -gt 0 -and -not $selfAncestry.Contains($procId) -and $parentByPid.ContainsKey($procId)) {
    [void]$victimRoots.Add($procId)
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
    $matchesVite = $isRepoScriptHost -and $cmd -and
      $cmd.IndexOf($nodeModules, [StringComparison]::OrdinalIgnoreCase) -ge 0 -and
      ($cmd -match 'vite[\\/]bin[\\/]vite\.js|@tauri-apps[\\/]cli[\\/]tauri\.js') -and
      ($cmd.IndexOf(":$VitePort", [StringComparison]::OrdinalIgnoreCase) -ge 0 -or
        $cmd.IndexOf("--port $VitePort", [StringComparison]::OrdinalIgnoreCase) -ge 0 -or
        $cmd.IndexOf("CODETERM_VITE_PORT=$VitePort", [StringComparison]::OrdinalIgnoreCase) -ge 0)
    $matchesCdp = $cmd -and
      $cmd.IndexOf("--remote-debugging-port=$CdpPort", [StringComparison]::OrdinalIgnoreCase) -ge 0

    if ($matchesTarget -or $matchesInstanceConpty -or $matchesWebView -or $matchesVite -or $matchesCdp) {
      [void]$victimRoots.Add($procId)
    }
  }
  foreach ($port in @($ApiPort, $VitePort, $CdpPort)) {
    foreach ($listener in @($listeners | Where-Object { [int]$_.port -eq $port })) {
      [void]$victimRoots.Add([int]$listener.process_id)
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
  param([int] $ProcessId)
  if ($ProcessId -le 0 -or -not $selected.Add($ProcessId)) {
    return
  }
  if ($childrenByParent.ContainsKey($ProcessId)) {
    foreach ($child in $childrenByParent[$ProcessId]) {
      Add-ProcessTree -ProcessId ([int]$child.ProcessId)
    }
  }
}
foreach ($procId in @($victimRoots)) {
  Add-ProcessTree -ProcessId $procId
}

if ($PlanOnly) {
  [pscustomobject]@{
    instance = $Instance
    identifier = $Identifier
    api_port = $ApiPort
    vite_port = $VitePort
    cdp_port = $CdpPort
    victim_roots = @($victimRoots | Sort-Object)
    selected_processes = @($selected | Sort-Object)
  } | ConvertTo-Json -Depth 4
  exit 0
}

$seen = @{}
function Stop-ProcessTree {
  param([int] $ProcessId)
  if ($ProcessId -le 0 -or $seen.ContainsKey($ProcessId)) {
    return
  }
  $seen[$ProcessId] = $true
  if ($childrenByParent.ContainsKey($ProcessId)) {
    foreach ($child in $childrenByParent[$ProcessId]) {
      Stop-ProcessTree -ProcessId ([int]$child.ProcessId)
    }
  }
  Stop-Process -Id $ProcessId -Force -Confirm:$false -ErrorAction SilentlyContinue
}
foreach ($procId in @($victimRoots)) {
  Stop-ProcessTree -ProcessId $procId
}
