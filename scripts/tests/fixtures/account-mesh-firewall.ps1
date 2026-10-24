$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/../../../src-tauri/windows/account-mesh-firewall.ps1"
$program = 'C:\Program Files\Domios\domios-server\domios-server.exe'
$parameters = Get-MeshFirewallParameters $program
$snapshot = $parameters.Clone()
$snapshot.Remove('Name')
$script:assertions = 0
$script:rule = $null
$script:admin = $false
$script:elevations = 0
$script:elevationResult = 1223
$script:writes = 0
$script:persist = $true

function Assert($Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
    $script:assertions++
}
function Get-NetFirewallRule { param($PolicyStore, $Name, $ErrorAction) return $script:rule }
function Get-NetFirewallApplicationFilter { return @{ Program = $script:rule.Program } }
function Get-NetFirewallPortFilter {
    return @{ Protocol = $script:rule.Protocol; LocalPort = $script:rule.LocalPort; RemotePort = $script:rule.RemotePort }
}
function Get-NetFirewallAddressFilter {
    return @{ LocalAddress = $script:rule.LocalAddress; RemoteAddress = $script:rule.RemoteAddress }
}
function Test-MeshFirewallAdministrator { return $script:admin }
function Invoke-MeshFirewallElevation {
    param($Action, $Program)
    Assert ($Action -in @('Ensure', 'Remove') -and $Program -eq $program) 'Elevation must retain the bounded operation and exact program'
    $script:elevations++
    return $script:elevationResult
}
function New-NetFirewallRule {
    param($Name, $DisplayName, $Group, $Description, $Program, $Direction, $Action, $Enabled, $Profile, $Protocol, $LocalPort, $RemotePort, $LocalAddress, $RemoteAddress, $EdgeTraversalPolicy, $PolicyStore)
    $script:writes++
    if ($script:persist) {
        $script:rule = @{}
        foreach ($key in $PSBoundParameters.Keys) { $script:rule[$key] = $PSBoundParameters[$key] }
    }
}
function Set-NetFirewallRule {
    [CmdletBinding()]
    param($Name, $NewDisplayName, $Description, $Program, $Direction, $Action, $Enabled, $Profile, $Protocol, $LocalPort, $RemotePort, $LocalAddress, $RemoteAddress, $EdgeTraversalPolicy, $PolicyStore)
    $script:writes++
    if ($script:persist) {
        $script:rule = @{}
        foreach ($key in $PSBoundParameters.Keys) { $script:rule[$key] = $PSBoundParameters[$key] }
    }
}
function Remove-NetFirewallRule {
    param($Name, $PolicyStore)
    $script:writes++
    if ($script:persist) { $script:rule = $null }
}

Assert ($parameters.Name -match '^Domios\.AccountMesh\.[A-F0-9]{64}$') 'Deterministic owned name'
Assert ((Get-MeshFirewallParameters $program.ToUpperInvariant()).Name -eq $parameters.Name) 'Path case is not a new installation'
Assert ((Get-MeshFirewallParameters 'D:\Domios\domios-server.exe').Name -ne $parameters.Name) 'Separate installs own separate rules'
Assert ((Invoke-MeshFirewall Check $program) -eq 2) 'Missing rule check'
Assert ($script:elevations -eq 0 -and $script:writes -eq 0) 'Check must be read-only'
Assert ((Invoke-MeshFirewall Remove $program) -eq 0) 'Absent uninstall skips elevation'
Assert ((Invoke-MeshFirewall Ensure $program) -eq 1223) 'UAC decline is preserved'
Assert ($script:writes -eq 0) 'Unelevated process never mutates firewall'
$script:admin = $true
Assert ((Invoke-MeshFirewall Ensure $program) -eq 0) 'Fresh elevated install succeeds'
Assert ($script:writes -eq 1) 'Fresh install creates once'
Assert ((Invoke-MeshFirewall Check $program) -eq 0) 'Persisted rule passes check'
$script:admin = $false
$beforeElevations = $script:elevations
Assert ((Invoke-MeshFirewall Ensure $program) -eq 0) 'Normal update succeeds'
Assert ($script:elevations -eq $beforeElevations -and $script:writes -eq 1) 'Normal update has no UAC or writes'
foreach ($entry in @(@('RemoteAddress', @('Any')), @('Protocol', 'UDP'), @('Profile', 'Private'), @('Program', 'C:\foreign.exe'), @('Enabled', 'False'))) {
    $saved = $script:rule[$entry[0]]
    $script:rule[$entry[0]] = $entry[1]
    Assert ((Invoke-MeshFirewall Check $program) -eq 2) "Reject mismatched $($entry[0])"
    $script:rule[$entry[0]] = $saved
}
$script:admin = $true
$script:rule.RemoteAddress = @('Any')
Assert ((Invoke-MeshFirewall Ensure $program) -eq 0 -and $script:writes -eq 2) 'Repair replaces overly broad parameters'
Assert ((Invoke-MeshFirewall Remove $program) -eq 0 -and -not $script:rule) 'Uninstall verifies removal'
$script:persist = $false
$refused = $false
try { Invoke-MeshFirewall Ensure $program | Out-Null } catch { $refused = $true }
Assert $refused 'Unpersisted install fails verification'
@{ command = $snapshot; assertions = $script:assertions } | ConvertTo-Json -Depth 4 -Compress
