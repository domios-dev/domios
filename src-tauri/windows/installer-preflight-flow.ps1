. "$PSScriptRoot/installer-process-model.ps1"

function Get-InstalledCaptureHolders {
    param([string]$Root, $Capture)
    if ($Capture.Table -isnot [hashtable] -or $null -eq $Capture.Holders) {
        throw 'Install-holder discovery is unavailable.'
    }
    $holders = @{}
    foreach ($record in $Capture.Table.Values) {
        if (Test-InstalledBinaryImage $Root $record.Image) {
            $holders["$($record.Pid)/$($record.Created)"] = $record
        }
    }
    foreach ($holder in $Capture.Holders) {
        if ($null -eq $holder) { throw 'Install-holder discovery is incomplete.' }
        $holders["$($holder.Pid)/$($holder.Created)"] = $holder
    }
    return @($holders.Values | Sort-Object Pid, Created)
}

function Assert-NoInstalledHolders {
    param([string]$Root, [hashtable]$Ports)
    $current = & $Ports.Capture $Root
    try {
        if (@(Get-InstalledCaptureHolders $Root $current).Count -ne 0) {
            throw 'Install files still have live or unproven holders; installation cancelled.'
        }
    } finally {
        & $Ports.Dispose $current
    }
}

function Invoke-InstallerPreflight {
    param([string]$InstallRoot, [hashtable]$Ports)
    $root = (Get-InstalledImagePath ([IO.Path]::GetFullPath($InstallRoot))).TrimEnd('\')
    $serverImage = "$root\domios-server\domios-server.exe"
    $desktopImage = "$root\domios-app.exe"
    $treeImages = @($serverImage, "$root\conpty\x64\OpenConsole.exe", "$root\conpty\arm64\OpenConsole.exe")
    $capture = & $Ports.Capture $root
    try {
        $holders = @(Get-InstalledCaptureHolders $root $capture)
        if ($holders.Count -eq 0) {
            Assert-NoInstalledHolders $root $Ports
            return $true
        }
        foreach ($holder in $holders) {
            if ($treeImages -notcontains $holder.Image -and $holder.Image -ine $desktopImage) {
                throw 'An install-file holder has unknown ownership; close it and retry.'
            }
        }
        if (@($holders | Where-Object { $_.Image -ieq $serverImage }).Count -eq 0) {
            throw 'An installed desktop or holder is running without a provable server.'
        }
        $token = & $Ports.Token $root
        $snapshot = & $Ports.Snapshot $token
        $serverPid = [int]$snapshot.ready.process.pid
        $serverCreated = [long]$snapshot.ready.process.created
        $server = $capture.Table[$serverPid]
        if ($null -eq $server -or $server.Image -ine $serverImage -or
            $server.Created -ne $serverCreated -or
            (Get-InstalledImagePath $snapshot.ready.process.executable) -ine $serverImage) {
            throw 'The running server does not belong to this install directory.'
        }
        $client = $null
        if ($null -ne $snapshot.client) {
            $client = $capture.Table[[int]$snapshot.client.process.pid]
            if ($null -eq $client -or $client.Image -ine $desktopImage -or
                $client.Created -ne [long]$snapshot.client.process.created -or
                (Get-InstalledImagePath $snapshot.client.process.executable) -ine $desktopImage) {
                throw 'The attached desktop does not belong to this install directory.'
            }
        }
        $owned = @()
        foreach ($holder in $holders) {
            $record = $capture.Table[[int]$holder.Pid]
            if ($null -eq $record -or $record.Created -ne $holder.Created -or $record.Image -ine $holder.Image) {
                throw 'An install-file holder identity is unproven.'
            }
            if ($holder.Image -ieq $desktopImage) {
                if ($null -eq $client -or $holder.Pid -ne $client.Pid -or $holder.Created -ne $client.Created) {
                    throw 'An installed desktop is not the attached client.'
                }
                continue
            }
            if (-not (Test-InstalledDescendant -Table $capture.Table -Images $treeImages -RootPid $serverPid -RootCreated $serverCreated -SubjectPid $holder.Pid -SubjectCreated $holder.Created)) {
                throw 'An installed process has unproven ancestry; close that instance and retry.'
            }
            $owned += $record
        }
        $saved = & $Ports.Save $token
        if ($saved.ok -ne $true) { throw 'Domios did not persist restore state; installation cancelled.' }
        $reply = & $Ports.Quit $token $snapshot
        if (-not (Test-InstallerCanStopTree -SnapshotSaved $saved.ok -QuitOutcome $reply.outcome)) {
            throw 'Domios refused graceful quit; installation cancelled.'
        }
        foreach ($record in $owned) { & $Ports.Terminate $record }
        $clock = [Diagnostics.Stopwatch]::StartNew()
        foreach ($record in $owned) {
            $remaining = [Math]::Max(0, 15000 - [int]$clock.ElapsedMilliseconds)
            if (-not (& $Ports.Wait $record $remaining)) { throw 'Owned install process exit deadline.' }
        }
        $remaining = [Math]::Max(0, 15000 - [int]$clock.ElapsedMilliseconds)
        if ($null -ne $client -and -not (& $Ports.Wait $client $remaining)) { throw 'Desktop exit deadline.' }
        Assert-NoInstalledHolders $root $Ports
        return $true
    } finally {
        & $Ports.Dispose $capture
    }
}
