function Get-DevCleanupProcess($row, [scriptblock] $open, [scriptblock] $validate) {
  $proc = & $open $row.pid
  if ($null -eq $proc) { return $null }
  try {
    if ($proc.WaitForExit(0)) { return $null }
    try { & $validate $proc $row }
    catch {
      if ($proc.WaitForExit(10000)) { return $null }
      throw
    }
    $result = $proc
    $proc = $null
    return $result
  } finally { if ($null -ne $proc) { $proc.Dispose() } }
}
