param([Parameter(Mandatory = $true)][string]$InstallRoot, [switch]$Development)
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/installer-preflight-flow.ps1"
Add-Type -Path "$PSScriptRoot/installer-process-native.cs"

$target = $null
if ($Development) {
    $root = [IO.Path]::GetFullPath($InstallRoot).TrimEnd('\')
    $file = Join-Path $root '.domios-isolated-update.json'
    if (-not [IO.File]::Exists($file)) {
        if ([IO.File]::Exists((Join-Path $root 'domios-dev-app.exe'))) {
            throw 'The installed DEV pair has no isolated update context.'
        }
    } else {
        $entry = Get-Item -LiteralPath $file
        if ($entry.Length -gt 8192 -or ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw 'Invalid isolated update context file.'
        }
        $launch = Get-Content -LiteralPath $file -Raw | ConvertFrom-Json
        $fields = @('install_root', 'instance', 'data_dir', 'config_dir', 'port', 'fixture_root')
        if (@($launch.PSObject.Properties).Count -ne $fields.Count -or
            @($launch.PSObject.Properties.Name | Where-Object { $_ -cnotin $fields }).Count -ne 0) {
            throw 'Unexpected isolated update context fields.'
        }
        $port = [int]$launch.port
        if ($launch.instance -cnotmatch '^[a-z0-9-]{1,64}$' -or $port -lt 1024 -or $port -gt 65535 -or $port -in @(7685, 7686) -or
            (Get-InstalledImagePath ([IO.Path]::GetFullPath($launch.install_root))).TrimEnd('\') -ine (Get-InstalledImagePath $root)) {
            throw 'Isolated update target does not match this install root.'
        }
        foreach ($directory in @($launch.data_dir, $launch.config_dir, $launch.fixture_root)) {
            if (-not [IO.Path]::IsPathRooted($directory) -or -not [IO.Directory]::Exists($directory) -or
                ((Get-Item -LiteralPath $directory).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
                throw 'Isolated update directories must be existing private directories.'
            }
            $ancestor = Get-Item -LiteralPath $directory
            while ($null -ne $ancestor) {
                if ($ancestor.Attributes -band [IO.FileAttributes]::ReparsePoint) {
                    throw 'Isolated update directories must not traverse links or junctions.'
                }
                $ancestor = $ancestor.Parent
            }
        }
        $target = @{
            Port = $port; Instance = $launch.instance
            Data = (Get-InstalledImagePath ([IO.Path]::GetFullPath($launch.data_dir))).TrimEnd('\')
            Config = (Get-InstalledImagePath ([IO.Path]::GetFullPath($launch.config_dir))).TrimEnd('\')
        }
        $homeRoot = [IO.Path]::GetFullPath([Environment]::GetFolderPath('UserProfile')).TrimEnd('\')
        foreach ($directory in @($target.Data, $target.Config)) {
            $relative = $directory.Substring([Math]::Min($directory.Length, $homeRoot.Length)).TrimStart('\')
            if ($directory.StartsWith("$homeRoot\", [StringComparison]::OrdinalIgnoreCase) -and $relative -match '^\.codeterm|^\.domios') {
                throw 'Isolated update refuses owner Domios directories.'
            }
            if ($directory -ieq $root -or $directory.StartsWith("$root\", [StringComparison]::OrdinalIgnoreCase)) {
                throw 'Isolated update data must stay outside the install directory.'
            }
        }
        if ($target.Data -ieq $target.Config -or
            [IO.Path]::GetFullPath($launch.fixture_root).TrimEnd('\') -ine "$($target.Data)\update-fixtures") {
            throw 'Isolated update directory boundaries do not match.'
        }
    }
}
$apiOrigin = if ($null -eq $target) { 'http://127.0.0.1:7685' } else { "http://127.0.0.1:$($target.Port)" }

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
        if ($Development) {
            if ($null -eq $target) { throw 'Isolated update context is required to stop a running pair.' }
            $env:CODETERM_API_PORT = [string]$target.Port
            $env:CODETERM_API_URL = $apiOrigin
            $env:CODETERM_INSTANCE = $target.Instance
            $env:CODETERM_SOCKET_PATH = "domios-dev-$($target.Instance).sock"
            $env:CODETERM_DATA_DIR = $target.Data
            $env:CODETERM_CONFIG_DIR = $target.Config
        }
        $cli = "$root/cli/codeterm.exe"
        if (-not (Test-Path -LiteralPath $cli)) { throw 'Installed Domios CLI unavailable.' }
        $token = (& $cli system api-token | Out-String).Trim()
        if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($token)) { throw 'Domios quit credential unavailable.' }
        return $token
    }
    Snapshot = {
        param($token)
        Invoke-RestMethod -Uri "$apiOrigin/api/app/quit/snapshot" -Headers @{ Authorization = "Bearer $token" } -TimeoutSec 10 -MaximumRedirection 0
    }
    Save = {
        param($token)
        Invoke-RestMethod -Method Post -Uri "$apiOrigin/api/session/save" -Headers @{ Authorization = "Bearer $token" } -TimeoutSec 30 -MaximumRedirection 0
    }
    Quit = {
        param($token, $snapshot)
        Invoke-RestMethod -Method Post -Uri "$apiOrigin/api/app/quit" -Headers @{ Authorization = "Bearer $token" } -ContentType 'application/json' -Body ($snapshot | ConvertTo-Json -Depth 20 -Compress) -TimeoutSec 30 -MaximumRedirection 0
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
    $null = Invoke-InstallerPreflight -InstallRoot $InstallRoot -Ports $ports -DevelopmentTarget $target
    exit 0
} catch {
    Write-Error "Domios installer preflight failed: $($_.Exception.Message)" -ErrorAction Continue
    exit 1
}
