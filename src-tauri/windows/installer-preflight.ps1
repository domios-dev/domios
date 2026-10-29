param([Parameter(Mandatory = $true)][string]$InstallRoot)
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/installer-preflight-flow.ps1"
Add-Type -Path "$PSScriptRoot/installer-process-native.cs"

function Get-DomiosInstallCapture {
    param([string]$Root)
    $files = [Collections.Generic.List[string]]::new()
    foreach ($relative in @('', 'cli', 'domios-server', 'conpty/x64', 'conpty/arm64')) {
        $directory = if ($relative -eq '') { $Root } else { Join-Path $Root $relative }
        if (-not [IO.Directory]::Exists($directory)) { continue }
        foreach ($file in Get-ChildItem -LiteralPath $directory -File) {
            if ($file.Extension -ieq '.exe' -or $file.Extension -ieq '.dll') { $files.Add($file.FullName) }
        }
    }
    $registered = @([DomiosInstallerProcess]::Holders($files.ToArray()))
    $registeredPids = @{}
    foreach ($holder in $registered) { $registeredPids[[int]$holder.Pid] = $true }
    $table = @{}
    $holders = @{}
    try {
        $captured = [DateTime]::UtcNow.ToFileTimeUtc()
        foreach ($entry in Get-CimInstance Win32_Process) {
            $process = $null
            try {
                $process = [Diagnostics.Process]::GetProcessById([int]$entry.ProcessId)
                $null = $process.Handle
                $created = $process.StartTime.ToUniversalTime().ToFileTimeUtc()
                $imageBuffer = [Text.StringBuilder]::new(32768)
                $imageSize = [uint32]32768
                if (-not [DomiosInstallerProcess]::QueryFullProcessImageName($process.Handle, 0, $imageBuffer, [ref]$imageSize)) {
                    throw 'Cannot resolve the pinned process image.'
                }
                $image = Get-InstalledImagePath $imageBuffer.ToString()
                if ($created -gt $captured) { throw 'Process identity changed during discovery.' }
                $record = @{
                    Pid = [int]$entry.ProcessId; Created = $created; ParentPid = [int]$entry.ParentProcessId
                    Image = $image; Process = $process
                }
                $table[$record.Pid] = $record
            } catch {
                if ($null -ne $process) { $process.Dispose() }
                if ($registeredPids.ContainsKey([int]$entry.ProcessId) -or (Test-InstalledBinaryImage $Root $entry.ExecutablePath)) {
                    throw 'Cannot pin an install-file holder; close Domios and retry.'
                }
            }
        }
        foreach ($holder in $registered) {
            $record = $table[[int]$holder.Pid]
            if ($null -eq $record -or $record.Created -ne [long]$holder.Created) {
                $record = @{ Pid = [int]$holder.Pid; Created = [long]$holder.Created; Image = $null }
            }
            $holders["$($holder.Pid)/$($holder.Created)"] = $record
        }
        return @{ Table = $table; Holders = @($holders.Values) }
    } catch {
        foreach ($record in $table.Values) { $record.Process.Dispose() }
        throw
    }
}

$ports = @{
    Capture = { param($root) Get-DomiosInstallCapture $root }
    Dispose = { param($capture) foreach ($record in $capture.Table.Values) { $record.Process.Dispose() } }
    Token = {
        param($root)
        # Installer authority must not inherit the launching tab's DEV selectors or credential.
        Get-ChildItem Env:CODETERM* | ForEach-Object { Remove-Item -LiteralPath "Env:$($_.Name)" }
        $env:CODETERM_API_PORT = '7685'
        $cli = "$root/cli/codeterm.exe"
        if (-not (Test-Path -LiteralPath $cli)) { throw 'Installed Domios CLI unavailable.' }
        $token = (& $cli system api-token | Out-String).Trim()
        if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($token)) { throw 'Domios quit credential unavailable.' }
        return $token
    }
    Snapshot = {
        param($token)
        Invoke-RestMethod -Uri 'http://127.0.0.1:7685/api/app/quit/snapshot' -Headers @{ Authorization = "Bearer $token" } -TimeoutSec 10 -MaximumRedirection 0
    }
    Save = {
        param($token)
        Invoke-RestMethod -Method Post -Uri 'http://127.0.0.1:7685/api/session/save' -Headers @{ Authorization = "Bearer $token" } -TimeoutSec 30 -MaximumRedirection 0
    }
    Quit = {
        param($token, $snapshot)
        Invoke-RestMethod -Method Post -Uri 'http://127.0.0.1:7685/api/app/quit' -Headers @{ Authorization = "Bearer $token" } -ContentType 'application/json' -Body ($snapshot | ConvertTo-Json -Depth 20 -Compress) -TimeoutSec 30 -MaximumRedirection 0
    }
    Terminate = {
        param($record)
        $process = $record.Process
        if (-not $process.HasExited -and -not [DomiosInstallerProcess]::TerminateProcess($process.Handle, 1) -and -not $process.HasExited) {
            throw 'An owned install process could not be stopped.'
        }
    }
    Wait = { param($record, $remaining) $record.Process.WaitForExit($remaining) }
}
try {
    $null = Invoke-InstallerPreflight -InstallRoot $InstallRoot -Ports $ports
    exit 0
} catch {
    Write-Error "Domios installer preflight failed: $($_.Exception.Message)" -ErrorAction Continue
    exit 1
}
