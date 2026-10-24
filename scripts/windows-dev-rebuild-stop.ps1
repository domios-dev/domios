param([ValidateSet('Inspect', 'Stop')][string] $Operation)
$ErrorActionPreference = 'Stop'
$request = [Console]::In.ReadToEnd() | ConvertFrom-Json
Add-Type -Path (Join-Path $PSScriptRoot 'windows-dev-rebuild-process.cs')

function Snapshot {
  $listeners = @(Get-NetTCPConnection -State Listen -ErrorAction Stop |
    ForEach-Object { [pscustomobject]@{ port = [int]$_.LocalPort; pid = [int]$_.OwningProcess } } |
    Sort-Object port, pid -Unique)
  $processes = @(Get-CimInstance Win32_Process | ForEach-Object {
    $created = ''
    $imageIdentity = ''
    try {
      $process = [Diagnostics.Process]::GetProcessById([int]$_.ProcessId)
      try { $created = $process.StartTime.ToUniversalTime().ToFileTimeUtc().ToString() } finally { $process.Dispose() }
      $imageIdentity = [DevRebuildProcess]::FileIdentity([string]$_.ExecutablePath)
    } catch {}
    [pscustomobject]@{ pid = [int]$_.ProcessId; parent = [int]$_.ParentProcessId; executable = [string]$_.ExecutablePath; created = $created; imageIdentity = $imageIdentity; commandLine = [string]$_.CommandLine }
  })
  return [pscustomobject]@{ processes = $processes; listeners = $listeners }
}

function Assert-Listeners($listeners, $ids, $allowed, $port) {
  $allowedListeners = @($allowed | ForEach-Object { "$($_.pid):$($_.port)" })
  if (@($listeners | Where-Object { $_.pid -in $ids -and
      ($_.port -eq 7685 -or "$($_.pid):$($_.port)" -notin $allowedListeners) }).Count) {
    throw 'DEV tree listener changed before shutdown; refusing'
  }
  if (@($listeners | Where-Object { $_.port -eq $port -and $_.pid -notin $ids }).Count) {
    throw 'DEV listener owner changed before shutdown; refusing'
  }
}

function Assert-ProcessIdentity($proc, $row) {
  if ($proc.Id -ne [int]$row.pid -or $proc.Created -cne $row.created -or
      -not $row.imageIdentity -or $proc.ImageIdentity -cne $row.imageIdentity) {
    throw 'DEV process changed before shutdown; refusing'
  }
}

if ($Operation -eq 'Inspect') { Snapshot | ConvertTo-Json -Depth 5 -Compress; exit 0 }
if ([int]$request.port -eq 7685 -or [int]$request.port -lt 1) { throw 'Refusing production or invalid DEV port' }
$pinned = @()
try {
  foreach ($row in $request.processes) {
    $proc = [DevRebuildProcess]::Open([int]$row.pid)
    if ($null -eq $proc) { continue }
    $pinned += $proc
    Assert-ProcessIdentity $proc $row
  }
  $snapshot = Snapshot
  $ids = @($pinned | ForEach-Object { $_.Id })
  Assert-Listeners $snapshot.listeners $ids $request.allowedListeners ([int]$request.port)
  try {
    Invoke-WebRequest -UseBasicParsing -Method Post -Uri "http://127.0.0.1:$($request.port)/api/app/quit" -ContentType 'application/json' -Headers @{ Authorization = "Bearer $($request.token)" } -Body ($request.quit | ConvertTo-Json -Depth 20 -Compress) -TimeoutSec 30 | Out-Null
  } catch { }
  foreach ($proc in $pinned) { [DevRebuildProcess]::Stop($proc, 10000) }
  $after = Snapshot
  if (@($after.listeners | Where-Object { $_.port -eq [int]$request.port }).Count) { throw 'DEV listener was replaced during shutdown; refusing staging' }
  '{"exited":true}'
} finally { foreach ($proc in $pinned) { $proc.Dispose() } }
