function Resolve-DevListenerOwner {
    param([object[]]$Listeners)
    $owners = @($Listeners | ForEach-Object { $_.OwningProcess } | Sort-Object -Unique)
    if ($owners.Count -ne 1 -or [uint32]$owners[0] -eq 0) {
        throw [System.InvalidOperationException]::new('Ambiguous DEV listener owner')
    }
    return [uint32]$owners[0]
}
