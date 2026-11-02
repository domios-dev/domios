. "$PSScriptRoot/installer-process-model.ps1"

function Get-InstalledCaptureHolders {
    param([string]$Root, $Capture)
    if ($Capture.Table -isnot [hashtable] -or $null -eq $Capture.Holders) {
        throw 'Install-holder discovery is unavailable.'
    }
    $holders = @{}
    foreach ($record in $Capture.Table.Values) {
        if (-not $record.Process.HasExited -and (Test-InstalledBinaryImage $Root $record.Image)) {
            $holders["$($record.Pid)/$($record.Created)"] = $record
        }
    }
    foreach ($holder in $Capture.Holders) {
        if ($null -eq $holder) { throw 'Install-holder discovery is incomplete.' }
        if (-not $holder.Process.HasExited) { $holders["$($holder.Pid)/$($holder.Created)"] = $holder }
    }
    return @($holders.Values | Sort-Object Pid, Created)
}

function Format-InstallHolder {
    param($Holder)
    return "$($Holder.Image) (PID $($Holder.Pid))"
}

function Invoke-InstallerPreflight {
    param([string]$InstallRoot, [hashtable]$Ports, [int]$GracefulMilliseconds = 10000, [int]$ExitMilliseconds = 15000)
    $root = (Get-InstalledImagePath ([IO.Path]::GetFullPath($InstallRoot))).TrimEnd('\')
    $clock = [Diagnostics.Stopwatch]::StartNew()
    $deadline = $GracefulMilliseconds + $ExitMilliseconds
    while ($true) {
        $capture = & $Ports.Capture $root
        try {
            $holders = @(Get-InstalledCaptureHolders $root $capture)
            if ($holders.Count -eq 0) { return $true }
            $foreign = @($holders | Where-Object { -not (Test-DomiosInstalledImage $root $_.Image) })
            if ($foreign.Count -ne 0) {
                throw "Close the install-file holders and choose Retry: $(($foreign | ForEach-Object { Format-InstallHolder $_ }) -join ', ')."
            }
            if ($clock.ElapsedMilliseconds -ge $deadline) {
                throw "Install-file holder exit deadline: $(($holders | ForEach-Object { Format-InstallHolder $_ }) -join ', ')."
            }
            foreach ($record in $holders) {
                $remaining = [Math]::Max(0, $GracefulMilliseconds - [int]$clock.ElapsedMilliseconds)
                $null = & $Ports.Wait $record $remaining
            }
            foreach ($record in $holders) { & $Ports.Terminate $record }
            foreach ($record in $holders) {
                $remaining = [Math]::Max(0, $deadline - [int]$clock.ElapsedMilliseconds)
                if (-not (& $Ports.Wait $record $remaining)) {
                    throw "Install-file holder exit deadline: $(Format-InstallHolder $record)."
                }
            }
        } finally { & $Ports.Dispose $capture }
    }
}
