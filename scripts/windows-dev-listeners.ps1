param([int] $Port, [string] $ServerImage)
$ErrorActionPreference = 'Stop'

function ConvertTo-DevTcpListeners($Connections, $NetstatLines) {
  $rows = @(
    foreach ($connection in $Connections) {
      $state = [string]$connection.State
      $unconnected = [int]$connection.RemotePort -eq 0 -and $connection.RemoteAddress -in @('0.0.0.0', '::', '[::]')
      if ($state -in @('Listen', '2') -or ($state -in @('', '0') -and $unconnected)) {
        if ([int]$connection.OwningProcess -gt 0 -and [int]$connection.LocalPort -gt 0 -and [int]$connection.LocalPort -le 65535) {
          [pscustomobject]@{ port = [int]$connection.LocalPort; pid = [int]$connection.OwningProcess }
        }
      }
    }
    foreach ($line in $NetstatLines) {
      if ($line -match '^\s*TCP\s+\S+:(\d+)\s+\S+\s+LISTEN(?:ING)?\s+(\d+)\s*$') {
        if ([int]$Matches[1] -gt 0 -and [int]$Matches[1] -le 65535 -and [int]$Matches[2] -gt 0) {
          [pscustomobject]@{ port = [int]$Matches[1]; pid = [int]$Matches[2] }
        }
      }
    }
  )
  return @($rows | Sort-Object port, pid -Unique)
}

function Get-DevTcpListeners {
  $connections = @()
  $providerError = $null
  try { $connections = @(Get-NetTCPConnection -ErrorAction Stop) } catch { $providerError = $_ }
  $netstatLines = @(& "$env:SystemRoot/System32/netstat.exe" -ano -p tcp)
  if ($LASTEXITCODE -ne 0 -and $providerError) { throw 'Windows TCP owner discovery unavailable' }
  return @(ConvertTo-DevTcpListeners $connections $netstatLines)
}

function Get-DevListenerPid([int] $SelectedPort, [string] $ExpectedServer) {
  if ($SelectedPort -lt 1 -or $SelectedPort -gt 65535 -or $SelectedPort -eq 7685) { throw 'Refusing production or invalid DEV port' }
  $owners = @(Get-DevTcpListeners | Where-Object port -eq $SelectedPort | Select-Object -ExpandProperty pid -Unique)
  if ($owners.Count -ne 1) { throw 'DEV listener owner is absent or ambiguous' }
  $owner = [Diagnostics.Process]::GetProcessById([int]$owners[0])
  try {
    [void]$owner.StartTime
    if (-not $ExpectedServer -or -not [string]::Equals($owner.MainModule.FileName,
        [IO.Path]::GetFullPath($ExpectedServer), [StringComparison]::OrdinalIgnoreCase)) {
      throw 'DEV listener executable does not match the selected server'
    }
    return $owner.Id
  } finally { $owner.Dispose() }
}

if ($MyInvocation.InvocationName -ne '.') { Get-DevListenerPid $Port $ServerImage }
