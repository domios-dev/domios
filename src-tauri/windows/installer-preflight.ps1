param([Parameter(Mandatory = $true)][string]$InstallRoot, [switch]$Development, [string]$TargetVersion)
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/installer-preflight-flow.ps1"
Add-Type -Path "$PSScriptRoot/installer-process-native.cs"

$target = $null
if ($Development) {
    $root = [IO.Path]::GetFullPath($InstallRoot).TrimEnd('\')
    $file = Join-Path $root '.domios-isolated-update.json'
    if (-not [IO.File]::Exists($file)) {
        if ([IO.File]::Exists((Join-Path $root 'domios-dev-app.exe'))) {
            throw 'The installed DEV pair has no isolated update context.'
        }
    } else {
        $entry = Get-Item -LiteralPath $file
        if ($entry.Length -gt 8192 -or ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw 'Invalid isolated update context file.'
        }
        $launch = Get-Content -LiteralPath $file -Raw | ConvertFrom-Json
        $fields = @('install_root', 'instance', 'data_dir', 'config_dir', 'port', 'fixture_root')
        if (@($fields | Where-Object { $_ -cnotin $launch.PSObject.Properties.Name }).Count -ne 0 -or
            @($launch.PSObject.Properties.Name | Where-Object { $_ -cnotin ($fields + @('diagnostics_port')) }).Count -ne 0) {
            throw 'Unexpected isolated update context fields.'
        }
        $port = [int]$launch.port
        if ('diagnostics_port' -cin $launch.PSObject.Properties.Name) {
            $diagnosticsPort = $launch.diagnostics_port
            if (($diagnosticsPort -isnot [long] -and $diagnosticsPort -isnot [int]) -or
                $diagnosticsPort -lt 1024 -or $diagnosticsPort -gt 65535 -or
                $diagnosticsPort -in @(7685, 7686, $port)) {
                throw 'Invalid isolated diagnostics port.'
            }
        }
        if ($launch.instance -cnotmatch '^[a-z0-9-]{1,64}$' -or $port -lt 1024 -or $port -gt 65535 -or $port -in @(7685, 7686) -or
            (Get-InstalledImagePath ([IO.Path]::GetFullPath($launch.install_root))).TrimEnd('\') -ine (Get-InstalledImagePath $root)) {
            throw 'Isolated update target does not match this install root.'
        }
        foreach ($directory in @($launch.data_dir, $launch.config_dir, $launch.fixture_root)) {
            if (-not [IO.Path]::IsPathRooted($directory) -or -not [IO.Directory]::Exists($directory) -or
                ((Get-Item -LiteralPath $directory).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
                throw 'Isolated update directories must be existing private directories.'
            }
            $ancestor = Get-Item -LiteralPath $directory
            while ($null -ne $ancestor) {
                if ($ancestor.Attributes -band [IO.FileAttributes]::ReparsePoint) {
                    throw 'Isolated update directories must not traverse links or junctions.'
                }
                $ancestor = $ancestor.Parent
            }
        }
        $target = @{
            Port = $port; Instance = $launch.instance
            Data = (Get-InstalledImagePath ([IO.Path]::GetFullPath($launch.data_dir))).TrimEnd('\')
            Config = (Get-InstalledImagePath ([IO.Path]::GetFullPath($launch.config_dir))).TrimEnd('\')
        }
        $homeRoot = [IO.Path]::GetFullPath([Environment]::GetFolderPath('UserProfile')).TrimEnd('\')
        foreach ($directory in @($target.Data, $target.Config)) {
            $relative = $directory.Substring([Math]::Min($directory.Length, $homeRoot.Length)).TrimStart('\')
            if ($directory.StartsWith("$homeRoot\", [StringComparison]::OrdinalIgnoreCase) -and $relative -match '^\.codeterm|^\.domios') {
                throw 'Isolated update refuses owner Domios directories.'
            }
            if ($directory -ieq $root -or $directory.StartsWith("$root\", [StringComparison]::OrdinalIgnoreCase)) {
                throw 'Isolated update data must stay outside the install directory.'
            }
        }
        if ($target.Data -ieq $target.Config -or
            [IO.Path]::GetFullPath($launch.fixture_root).TrimEnd('\') -ine "$($target.Data)\update-fixtures") {
            throw 'Isolated update directory boundaries do not match.'
        }
    }
}

. "$PSScriptRoot/installer-process-capture.ps1"

$ports = @{
    Capture = { param($root) Get-DomiosInstallCapture $root }
    Dispose = { param($capture) foreach ($record in $capture.Table.Values) { $record.Process.Dispose() } }
    Terminate = {
        param($record)
        $process = $record.Process
        if (-not $process.HasExited -and -not [DomiosInstallerProcess]::TerminateProcess($process.Handle, 1) -and -not $process.HasExited) {
            throw "Cannot stop install-file holder $(Format-InstallHolder $record)."
        }
    }
    Wait = { param($record, $remaining) $record.Process.WaitForExit($remaining) }
}
try {
    $null = Invoke-InstallerPreflight -InstallRoot $InstallRoot -Ports $ports
    $refusal = Join-Path $InstallRoot '.domios-installer-refusal.json'
    if ([IO.File]::Exists($refusal)) { Remove-Item -LiteralPath $refusal }
    exit 0
} catch {
    $message = "Domios installer preflight failed: $($_.Exception.Message)"
    try {
        @{ target_version = $TargetVersion; message = $message } | ConvertTo-Json -Compress |
            Set-Content -LiteralPath (Join-Path $InstallRoot '.domios-installer-refusal.json') -Encoding UTF8
    } catch { Write-Warning 'Could not retain installer refusal details.' }
    Write-Output $message
    exit 1
}
