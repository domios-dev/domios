function Get-InstalledImagePath {
    param([string]$Path)
    if ($Path.StartsWith('\\?\')) { return $Path.Substring(4) }
    return $Path
}

function Test-InstalledBinaryImage {
    param([string]$Root, [string]$Image)
    if ([string]::IsNullOrWhiteSpace($Image)) { return $false }
    $path = Get-InstalledImagePath $Image
    if ([IO.Path]::GetExtension($path) -ine '.exe') { return $false }
    $directories = @($Root, "$Root\cli", "$Root\domios-server", "$Root\conpty\x64", "$Root\conpty\arm64")
    return $directories -contains [IO.Path]::GetDirectoryName($path)
}

function Test-InstallerCanStopTree {
    param([bool]$SnapshotSaved, [string]$QuitOutcome)
    return $SnapshotSaved -and @('completed', 'uncertain') -contains $QuitOutcome
}

function Test-InstalledDescendant {
    param([hashtable]$Table, [string[]]$Images, [int]$RootPid, [long]$RootCreated, [int]$SubjectPid, [long]$SubjectCreated)
    $currentPid = $SubjectPid
    $created = $SubjectCreated
    for ($index = 0; $index -lt $Table.Count; $index++) {
        $record = $Table[$currentPid]
        if ($null -eq $record -or $record.Created -ne $created) { return $false }
        if ($Images -notcontains (Get-InstalledImagePath $record.Image)) { return $false }
        if ($currentPid -eq $RootPid -and $created -eq $RootCreated) { return $true }
        $parent = $Table[[int]$record.ParentPid]
        if ($null -eq $parent -or $parent.Created -ge $created) { return $false }
        $currentPid = [int]$record.ParentPid
        $created = $parent.Created
    }
    return $false
}
