param(
    [Parameter(Mandatory=$true)][string]$Session,
    [Parameter(Mandatory=$true)][string]$OutputFile,
    [uint32]$ExpectedPid,
    [string]$ExpectedCreation,
    [uint32]$ExpectedDevPid,
    [switch]$ForceExit
)
$ErrorActionPreference = 'Stop'
if ($env:CODETERM_API_PORT -ne '7686' -or -not $env:CODETERM_DEV_DATA_DIR) { throw 'dev_run required' }
if ($Session -notmatch '^[a-zA-Z0-9_-]+$') { throw 'Invalid exact session id' }
. (Join-Path $PSScriptRoot 'process-owner.ps1')
$devPid = Resolve-DevListenerOwner -Listeners @(Get-NetTCPConnection -State Listen -LocalPort 7686)
if ($ExpectedDevPid -and $devPid -ne $ExpectedDevPid) { throw 'DEV listener changed' }
$processes = @(Get-CimInstance Win32_Process)
$owned = Resolve-DevResumedProcess -Processes $processes -Session $Session -DevPid $devPid
@{ session = $Session; devPid = $devPid; process = $owned } | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $OutputFile -Encoding utf8
if ($ForceExit) {
    if (-not $ExpectedPid -or $owned.pid -ne $ExpectedPid -or $owned.created -ne $ExpectedCreation) { throw 'Expected PID and creation identity mismatch' }
    $live = Get-Process -Id $ExpectedPid
    $startTicks = $live.StartTime.ToUniversalTime().Ticks
    $expectedTicks = [DateTime]::Parse($ExpectedCreation).ToUniversalTime().Ticks
    if (($startTicks - $startTicks % 10) -ne ($expectedTicks - $expectedTicks % 10)) { throw 'PID identity changed before exit' }
    $live.Kill()
}
