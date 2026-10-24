param([ValidateSet('Inspect', 'Stop')][string] $Operation)
$ErrorActionPreference = 'Stop'
$request = [Console]::In.ReadToEnd() | ConvertFrom-Json

function Snapshot {
  $listeners = @(Get-NetTCPConnection -State Listen -ErrorAction Stop |
    ForEach-Object { [pscustomobject]@{ port = [int]$_.LocalPort; pid = [int]$_.OwningProcess } } |
    Sort-Object port, pid -Unique)
  $processes = @(Get-CimInstance Win32_Process | ForEach-Object {
    $created = ''
    try {
      $process = [Diagnostics.Process]::GetProcessById([int]$_.ProcessId)
      try { $created = $process.StartTime.ToUniversalTime().ToFileTimeUtc().ToString() } finally { $process.Dispose() }
    } catch {}
    [pscustomobject]@{ pid = [int]$_.ProcessId; parent = [int]$_.ParentProcessId; executable = [string]$_.ExecutablePath; created = $created }
  })
  return [pscustomobject]@{ processes = $processes; listeners = $listeners }
}

if ($Operation -eq 'Inspect') { Snapshot | ConvertTo-Json -Depth 5 -Compress; exit 0 }
if ([int]$request.port -eq 7685 -or [int]$request.port -lt 1) { throw 'Refusing production or invalid DEV port' }
$pinned = @()
try {
  foreach ($row in $request.processes) {
    try { $proc = [Diagnostics.Process]::GetProcessById([int]$row.pid) } catch [ArgumentException] { continue }
    [void]$proc.Handle
    if ($proc.StartTime.ToUniversalTime().ToFileTimeUtc().ToString() -ne $row.created -or
        $proc.MainModule.FileName -ine ([string]$row.executable -replace '^\\\\\?\\', '')) {
      $proc.Dispose()
      throw 'DEV process changed before shutdown; refusing'
    }
    $pinned += $proc
  }
  $snapshot = Snapshot
  $ids = @($pinned | ForEach-Object { $_.Id })
  if (@($snapshot.listeners | Where-Object { $_.pid -in $ids -and $_.port -notin $request.allowedPorts }).Count) { throw 'DEV tree owns another listener; refusing' }
  $graceful = New-Object Diagnostics.Process
  try {
    $graceful.StartInfo.FileName = [string]$request.cli
    $graceful.StartInfo.Arguments = 'app quit --exact --json'
    $graceful.StartInfo.UseShellExecute = $false
    $graceful.StartInfo.CreateNoWindow = $true
    $graceful.StartInfo.RedirectStandardOutput = $true
    $graceful.StartInfo.RedirectStandardError = $true
    $graceful.StartInfo.EnvironmentVariables['CODETERM_DATA_DIR'] = [string]$request.data
    $graceful.StartInfo.EnvironmentVariables['CODETERM_API_URL'] = "http://127.0.0.1:$($request.port)"
    $graceful.StartInfo.EnvironmentVariables['CODETERM_API_PORT'] = [string]$request.port
    $graceful.StartInfo.EnvironmentVariables['CODETERM_API_TOKEN'] = [string]$request.token
    if (-not $graceful.Start()) { throw 'DEV graceful quit did not start' }
    $output = $graceful.StandardOutput.ReadToEndAsync()
    $errors = $graceful.StandardError.ReadToEndAsync()
    if (-not $graceful.WaitForExit(30000)) { $graceful.Kill(); $graceful.WaitForExit() }
  } finally { $graceful.Dispose() }
  foreach ($proc in $pinned) { if (-not $proc.HasExited) { $proc.Kill() } }
  foreach ($proc in $pinned) { if (-not $proc.WaitForExit(10000)) { throw 'DEV pinned process did not exit; refusing staging' } }
  '{"exited":true}'
} finally { foreach ($proc in $pinned) { $proc.Dispose() } }
