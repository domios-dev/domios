function Get-InstallerPinnedProcess {
    param($Entry)
    $process = $null
    try {
        $process = [Diagnostics.Process]::GetProcessById([int]$Entry.ProcessId)
        $null = $process.Handle
        if ($process.HasExited) { $process.Dispose(); return $null }
        $created = $process.StartTime.ToUniversalTime().ToFileTimeUtc()
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
                if ($registeredPids.ContainsKey($processId) -or (Test-InstalledBinaryImage $Root $entry.ExecutablePath)) {
                    throw "Cannot pin install-file holder $($entry.ExecutablePath) (PID $processId): $($_.Exception.Message)"
                }
            }
        }
        foreach ($holder in $registered) {
            $record = $table[[int]$holder.Pid]
            if ($null -eq $record -or $record.Created -ne [long]$holder.Created) {
                if ((& $ports.State ([int]$holder.Pid) ([long]$holder.Created)) -in @('Exited', 'Replaced')) { continue }
                throw "Cannot pin install-file holder $($holder.Name) (PID $($holder.Pid))."
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
