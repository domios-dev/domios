function Get-InstallerPinnedProcess {
    param($Entry)
    $process = $null
    try {
        $process = [Diagnostics.Process]::GetProcessById([int]$Entry.ProcessId)
        $null = $process.Handle
        if ($process.HasExited) { $process.Dispose(); return $null }
        $startTime = $process.StartTime
        if ($null -eq $startTime) {
            throw 'Access denied reading process start time.'
        }
        $created = $startTime.ToUniversalTime().ToFileTimeUtc()
        if ($null -ne $Entry.CreationDate -and ($created - ($created % 10)) -ne $Entry.CreationDate.ToUniversalTime().ToFileTimeUtc()) {
            $process.Dispose()
            return $null
        }
        $imageBuffer = [Text.StringBuilder]::new(32768)
        $imageSize = [uint32]32768
        if (-not [DomiosInstallerProcess]::QueryFullProcessImageName($process.Handle, 0, $imageBuffer, [ref]$imageSize)) {
            throw 'Cannot resolve the pinned process image.'
        }
        if ($process.HasExited) { $process.Dispose(); return $null }
        return @{
            Pid = [int]$Entry.ProcessId; Created = $created; ParentPid = [int]$Entry.ParentProcessId
            Image = (Get-InstalledImagePath $imageBuffer.ToString()); Process = $process
        }
    } catch {
        if ($null -ne $process) {
            try { if ($process.HasExited) { return $null } } finally { $process.Dispose() }
        }
        throw
    }
}

function Get-DomiosInstallCapture {
    param([string]$Root, [hashtable]$Discovery = @{})
    $ports = @{
        Entries = { @(Get-CimInstance Win32_Process) }
        Registered = {
            param($root)
            $files = [Collections.Generic.List[string]]::new()
            foreach ($relative in @('', 'cli', 'domios-server', 'conpty/x64', 'conpty/arm64')) {
                $directory = if ($relative -eq '') { $root } else { Join-Path $root $relative }
                if (-not [IO.Directory]::Exists($directory)) { continue }
                foreach ($file in Get-ChildItem -LiteralPath $directory -File) {
                    if ($file.Extension -ieq '.exe' -or $file.Extension -ieq '.dll') { $files.Add($file.FullName) }
                }
            }
            @([DomiosInstallerProcess]::Holders($files.ToArray()))
        }
        Pin = { param($entry) Get-InstallerPinnedProcess $entry }
        State = { param($pidValue, $created) [DomiosInstallerProcess]::State($pidValue, $created) }
    }
    foreach ($key in $Discovery.Keys) { $ports[$key] = $Discovery[$key] }
    $registered = @(& $ports.Registered $Root)
    $registeredPids = @{}
    foreach ($holder in $registered) { $registeredPids[[int]$holder.Pid] = $holder }
    $table = @{}
    $holders = @{}
    try {
        foreach ($entry in (& $ports.Entries)) {
            $processId = [int]$entry.ProcessId
            try {
                $record = & $ports.Pin $entry
                if ($null -ne $record) { $table[$processId] = $record }
            } catch {
                $created = 0L
                if ($registeredPids.ContainsKey($processId)) { $created = [long]$registeredPids[$processId].Created }
                if ((& $ports.State $processId $created) -in @('Exited', 'Replaced')) { continue }
                $isInstalled = Test-InstalledBinaryImage $Root $entry.ExecutablePath
                if ($registeredPids.ContainsKey($processId) -or $isInstalled) {
                    $holderName = if (-not [string]::IsNullOrWhiteSpace($entry.ExecutablePath)) {
                        $entry.ExecutablePath
                    } elseif ($registeredPids.ContainsKey($processId) -and -not [string]::IsNullOrWhiteSpace($registeredPids[$processId].Name)) {
                        $registeredPids[$processId].Name
                    } else {
                        $entry.Name
                    }
                    $err = [InvalidOperationException]::new("Cannot pin install-file holder $holderName (PID $processId): $($_.Exception.Message)")
                    if (-not $isInstalled) { $err.Data['TransientUnpinnableHolder'] = $true }
                    throw $err
                }
            }
        }
        foreach ($holder in $registered) {
            $record = $table[[int]$holder.Pid]
            if ($null -eq $record -or $record.Created -ne [long]$holder.Created) {
                if ((& $ports.State ([int]$holder.Pid) ([long]$holder.Created)) -in @('Exited', 'Replaced')) { continue }
                $err = [InvalidOperationException]::new("Cannot pin install-file holder $($holder.Name) (PID $($holder.Pid)).")
                $err.Data['TransientUnpinnableHolder'] = $true
                throw $err
            }
            if ($record.Process.HasExited) { continue }
            $holders["$($holder.Pid)/$($holder.Created)"] = $record
        }
        return @{ Table = $table; Holders = @($holders.Values) }
    } catch {
        foreach ($record in $table.Values) { $record.Process.Dispose() }
        throw
    }
}
