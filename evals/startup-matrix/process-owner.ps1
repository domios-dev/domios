function Resolve-DevListenerOwner {
    param([object[]]$Listeners)
    $owners = @($Listeners | ForEach-Object { $_.OwningProcess } | Sort-Object -Unique)
    if ($owners.Count -ne 1 -or [uint32]$owners[0] -eq 0) {
        throw [System.InvalidOperationException]::new('Ambiguous DEV listener owner')
    }
    return [uint32]$owners[0]
}

function Resolve-DevResumedProcess {
    param([object[]]$Processes, [string]$Session, [uint32]$DevPid)
    if (-not $DevPid -or $Session -notmatch '^[a-zA-Z0-9_-]+$') { throw 'Exact session and DEV owner required' }
    $pattern = '(?:^|\s)--resume(?:=|\s+)["'']?' + [regex]::Escape($Session) + '(?:["'']|\s|$)'
    $candidates = @()
    foreach ($process in @($Processes | Where-Object { $_.CommandLine -and $_.CommandLine -match $pattern })) {
        $current = $process
        $seen = @{}
        $chain = @()
        while ($current -and -not $seen.ContainsKey([string]$current.ProcessId)) {
            $seen[[string]$current.ProcessId] = $true
            $chain += [uint32]$current.ProcessId
            if ($current.ProcessId -eq $DevPid) { break }
            $parentId = $current.ParentProcessId
            $current = $Processes | Where-Object ProcessId -eq $parentId | Select-Object -First 1
        }
        if ($current -and $current.ProcessId -eq $DevPid -and $process.ProcessId -ne $DevPid) {
            $candidates += @{ pid = [uint32]$process.ProcessId; created = $process.CreationDate.ToUniversalTime().ToString('o'); chain = $chain }
        }
    }
    if ($candidates.Count -ne 1) { throw 'Exactly one resumed process descending from DEV required' }
    return $candidates[0]
}
