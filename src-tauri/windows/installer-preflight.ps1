param([Parameter(Mandatory = $true)][string]$InstallRoot)
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/installer-process-model.ps1"

Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class DomiosInstallerProcess {
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool TerminateProcess(IntPtr process, uint code);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern bool QueryFullProcessImageName(IntPtr process, uint flags, StringBuilder image, ref uint size);
}
'@

$root = (Get-InstalledImagePath ([IO.Path]::GetFullPath($InstallRoot))).TrimEnd('\')
$images = @(
    "$root\domios-server\domios-server.exe",
    "$root\conpty\x64\OpenConsole.exe",
    "$root\conpty\arm64\OpenConsole.exe"
)
$table = @{}
$owned = @()
try {
    $captured = [DateTime]::UtcNow.ToFileTimeUtc()
    foreach ($entry in Get-CimInstance Win32_Process) {
        $process = $null
        try {
            $process = [Diagnostics.Process]::GetProcessById([int]$entry.ProcessId)
            $null = $process.Handle
            $created = $process.StartTime.ToUniversalTime().ToFileTimeUtc()
            if ($created -gt $captured) { $process.Dispose(); continue }
            $imageBuffer = [Text.StringBuilder]::new(32768)
            $imageSize = [uint32]32768
            if (-not [DomiosInstallerProcess]::QueryFullProcessImageName($process.Handle, 0, $imageBuffer, [ref]$imageSize)) {
                throw 'Cannot resolve the pinned process image.'
            }
            $image = Get-InstalledImagePath $imageBuffer.ToString()
            $table[[int]$entry.ProcessId] = @{
                Created = $created; ParentPid = [int]$entry.ParentProcessId
                Image = $image; Process = $process
            }
        } catch {
            if ($null -ne $process) { $process.Dispose() }
            if ($images -contains $entry.ExecutablePath) { throw 'Cannot pin an installed process; close Domios and retry.' }
        }
    }
    $candidates = @($table.Values | Where-Object { $images -contains $_.Image })
    if ($candidates.Count -eq 0) { exit 0 }

    # Installer authority must not inherit the launching tab's DEV selectors or credential.
    Get-ChildItem Env:CODETERM* | ForEach-Object { Remove-Item -LiteralPath "Env:$($_.Name)" }
    $env:CODETERM_API_PORT = '7685'
    $cli = "$root\cli\codeterm.exe"
    if (-not (Test-Path -LiteralPath $cli)) { throw 'Installed Domios CLI unavailable; close Domios and retry.' }
    $token = (& $cli system api-token | Out-String).Trim()
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($token)) { throw 'Domios quit credential unavailable.' }
    $headers = @{ Authorization = "Bearer $token" }
    $base = 'http://127.0.0.1:7685/api/app/quit'
    $snapshot = Invoke-RestMethod -Uri "$base/snapshot" -Headers $headers -TimeoutSec 10 -MaximumRedirection 0
    $serverPid = [int]$snapshot.ready.process.pid
    $serverCreated = [long]$snapshot.ready.process.created
    $server = $table[$serverPid]
    if ($null -eq $server -or $server.Image -ine $images[0] -or
        $server.Created -ne $serverCreated -or (Get-InstalledImagePath $snapshot.ready.process.executable) -ine $images[0]) {
        throw 'The running server does not belong to this install directory.'
    }
    if ($null -ne $snapshot.client) {
        $client = $table[[int]$snapshot.client.process.pid]
        if ($null -eq $client -or $client.Image -ine "$root\domios-app.exe" -or
            $client.Created -ne [long]$snapshot.client.process.created -or
            (Get-InstalledImagePath $snapshot.client.process.executable) -ine "$root\domios-app.exe") {
            throw 'The attached desktop does not belong to this install directory.'
        }
    }
    foreach ($candidate in $candidates) {
        if (-not (Test-InstalledDescendant -Table $table -Images $images -RootPid $serverPid -RootCreated $serverCreated -SubjectPid $candidate.Process.Id -SubjectCreated $candidate.Created)) {
            throw 'An installed process has unproven ancestry; close that instance and retry.'
        }
        $owned += $candidate.Process
    }
    $saved = Invoke-RestMethod -Method Post -Uri 'http://127.0.0.1:7685/api/session/save' -Headers $headers -TimeoutSec 30 -MaximumRedirection 0
    if ($saved.ok -ne $true) { throw 'Domios did not persist restore state; installation cancelled.' }
    $quitOutcome = 'transport_error'
    try {
        $reply = Invoke-RestMethod -Method Post -Uri $base -Headers $headers -ContentType 'application/json' -Body ($snapshot | ConvertTo-Json -Depth 20 -Compress) -TimeoutSec 30 -MaximumRedirection 0
        $quitOutcome = $reply.outcome
    } catch {
        Write-Output 'Graceful quit did not confirm exit; restore state is saved. Checking the pinned install tree.'
    }
    if (-not (Test-InstallerCanStopTree -SnapshotSaved $saved.ok -QuitOutcome $quitOutcome)) {
        throw 'Domios refused graceful quit; installation cancelled.'
    }

    foreach ($process in $owned) {
        if (-not $process.HasExited -and -not [DomiosInstallerProcess]::TerminateProcess($process.Handle, 1) -and -not $process.HasExited) {
            throw 'An owned install process could not be stopped.'
        }
    }
    $exitClock = [Diagnostics.Stopwatch]::StartNew()
    foreach ($process in $owned) {
        $remaining = [Math]::Max(0, 15000 - [int]$exitClock.ElapsedMilliseconds)
        if (-not $process.WaitForExit($remaining)) { throw 'Owned install process exit deadline.' }
    }
    $remaining = [Math]::Max(0, 15000 - [int]$exitClock.ElapsedMilliseconds)
    if ($null -ne $snapshot.client -and -not $client.Process.WaitForExit($remaining)) { throw 'Desktop exit deadline.' }
    if (@(Get-CimInstance Win32_Process | Where-Object { $images -contains $_.ExecutablePath }).Count -ne 0) {
        throw 'Install processes are still running; retry after closing Domios.'
    }
    exit 0
} catch {
    Write-Error "Domios installer preflight failed: $($_.Exception.Message)" -ErrorAction Continue
    exit 1
} finally {
    foreach ($record in $table.Values) { $record.Process.Dispose() }
}
