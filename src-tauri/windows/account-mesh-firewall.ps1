param(
    [ValidateSet('Check', 'Ensure', 'Remove')]
    [string]$Operation = 'Check',
    [string]$ServerProgram
)

$ErrorActionPreference = 'Stop'

function Get-MeshFirewallParameters([string]$Program) {
    $digest = [System.Security.Cryptography.SHA256]::Create()
    try {
        $hash = [BitConverter]::ToString($digest.ComputeHash([Text.Encoding]::UTF8.GetBytes($Program.ToLowerInvariant()))).Replace('-', '')
    } finally { $digest.Dispose() }
    return @{
        Name = "Domios.AccountMesh.$hash"
        DisplayName = 'Domios account mesh (Tailscale only)'
        Group = 'Domios installer'
        Description = 'Installer-owned access for the Domios server from Tailscale peers.'
        Program = $Program
        Direction = 'Inbound'
        Action = 'Allow'
        Enabled = 'True'
        Profile = 'Any'
        Protocol = 'TCP'
        LocalPort = 'Any'
        RemotePort = 'Any'
        LocalAddress = 'Any'
        RemoteAddress = @('100.64.0.0/10', 'fd7a:115c:a1e0::/48')
        EdgeTraversalPolicy = 'Block'
        PolicyStore = 'PersistentStore'
    }
}

function Get-MeshFirewallRule($Parameters) {
    return Get-NetFirewallRule -PolicyStore PersistentStore -Name $Parameters.Name -ErrorAction SilentlyContinue
}

function Test-MeshFirewallRule($Rule, $Parameters) {
    if (-not $Rule -or @($Rule).Count -ne 1) { return $false }
    $app = $Rule | Get-NetFirewallApplicationFilter
    $port = $Rule | Get-NetFirewallPortFilter
    $address = $Rule | Get-NetFirewallAddressFilter
    $remote = @($address.RemoteAddress | ForEach-Object {
        $_.ToLowerInvariant().Replace('100.64.0.0/255.192.0.0', '100.64.0.0/10')
    } | Sort-Object)
    return $app.Program -ieq $Parameters.Program -and
        "$($Rule.Direction)" -eq 'Inbound' -and "$($Rule.Action)" -eq 'Allow' -and
        "$($Rule.Enabled)" -eq 'True' -and "$($Rule.Profile)" -eq 'Any' -and
        "$($Rule.EdgeTraversalPolicy)" -eq 'Block' -and
        "$($port.Protocol)" -in @('TCP', '6') -and
        "$($port.LocalPort)" -eq 'Any' -and "$($port.RemotePort)" -eq 'Any' -and
        "$($address.LocalAddress)" -eq 'Any' -and
        ($remote -join ',') -eq '100.64.0.0/10,fd7a:115c:a1e0::/48'
}

function Test-MeshFirewallAdministrator {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Invoke-MeshFirewallElevation([string]$Action, [string]$Program) {
    $arguments = '-NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "{0}" -Operation {1} -ServerProgram "{2}"' -f $PSCommandPath, $Action, $Program
    try {
        $process = Start-Process -FilePath "$PSHOME\powershell.exe" -ArgumentList $arguments -Verb RunAs -WindowStyle Hidden -Wait -PassThru
        return $process.ExitCode
    } catch {
        $cause = $_.Exception
        while ($cause) {
            if ($cause -is [ComponentModel.Win32Exception] -and $cause.NativeErrorCode -eq 1223) { return 1223 }
            $cause = $cause.InnerException
        }
        throw
    }
}

function Invoke-MeshFirewall([string]$Action, [string]$Program) {
    $parameters = Get-MeshFirewallParameters $Program
    $rule = Get-MeshFirewallRule $parameters
    if ($Action -eq 'Remove' -and -not $rule) { return 0 }
    if ($Action -ne 'Remove' -and (Test-MeshFirewallRule $rule $parameters)) { return 0 }
    if ($Action -eq 'Check') { return 2 }
    if (-not (Test-MeshFirewallAdministrator)) {
        return Invoke-MeshFirewallElevation $Action $Program
    }
    if ($Action -eq 'Remove') {
        $app = $rule | Get-NetFirewallApplicationFilter
        if ($app.Program -ine $Program) { throw 'Refusing to remove a rule for another executable.' }
        Remove-NetFirewallRule -PolicyStore PersistentStore -Name $parameters.Name
        if (Get-MeshFirewallRule $parameters) { throw 'Firewall rule removal did not persist.' }
    } else {
        if ($rule) {
            $repair = $parameters.Clone()
            $repair.Remove('Group')
            $repair.Remove('DisplayName')
            $repair.NewDisplayName = $parameters.DisplayName
            Set-NetFirewallRule @repair | Out-Null
        } else { New-NetFirewallRule @parameters | Out-Null }
        if (-not (Test-MeshFirewallRule (Get-MeshFirewallRule $parameters) $parameters)) {
            throw 'Firewall rule verification failed.'
        }
    }
    return 0
}

if ($MyInvocation.InvocationName -ne '.') {
    try {
        $expected = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../domios-server/domios-server.exe'))
        if (-not $ServerProgram) { $ServerProgram = $expected }
        if (-not [IO.Path]::IsPathRooted($ServerProgram) -or $ServerProgram -match '["\r\n]' -or
            [IO.Path]::GetFullPath($ServerProgram) -ine $expected) {
            throw 'The firewall helper only manages its installed Domios server.'
        }
        exit (Invoke-MeshFirewall $Operation $expected)
    } catch {
        [Console]::Error.WriteLine($_.Exception.Message)
        exit 1
    }
}
