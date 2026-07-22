# Install codeterm from a local NSIS installer, replacing the running daemon.
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File scripts\self-install-local.ps1 -Installer <path>
#
# -Installer  Absolute path to CodeTerm_X.Y.Z_x64-setup.exe.
# The installed application always relaunches after a successful install.

param(
    [Parameter(Mandatory=$true)]
    [string]$Installer,

    [switch]$Worker,

    [string]$LogPath
)

$ErrorActionPreference = 'Stop'

if (-not $LogPath) {
    $LogPath = Join-Path $env:TEMP 'codeterm-self-install.log'
}

if (-not (Test-Path $Installer)) {
    Write-Error "Installer not found: $Installer"
    exit 1
}

$scriptPath = $PSCommandPath
if (-not $Worker) {
    $outLog = "$LogPath.out"
    $errLog = "$LogPath.err"
    Remove-Item -LiteralPath $outLog, $errLog -ErrorAction SilentlyContinue
    $args = @(
        '-NoProfile',
        '-ExecutionPolicy', 'Bypass',
        '-File', "`"$scriptPath`"",
        '-Installer', "`"$((Resolve-Path $Installer).Path)`"",
        '-Worker',
        '-LogPath', "`"$LogPath`""
    )
    Write-Host "[self-install] Spawning detached installer helper..."
    Start-Process -FilePath 'powershell.exe' -ArgumentList $args -WindowStyle Hidden -RedirectStandardOutput $outLog -RedirectStandardError $errLog | Out-Null
    Write-Host "[self-install] Helper started. This CodeTerm instance will restart."
    Write-Host "[self-install] Logs: $outLog / $errLog"
    exit 0
}

$expectedExe = Join-Path $env:LocalAppData 'codeterm\codeterm.exe'

Write-Host "[self-install] Installer: $Installer"
Write-Host "[self-install] Target:    $expectedExe"

# 1. Stop any running codeterm daemon — NSIS in /S mode does NOT kill the
#    in-use binary on Windows, leading to "in-use" rename failures or, worse,
#    a silent half-install where files are out-of-sync with the running PID.
Get-Process codeterm -ErrorAction SilentlyContinue | ForEach-Object {
    Write-Host "[self-install] Stopping codeterm PID $($_.Id) ..."
    $_.Kill()
    $_.WaitForExit(5000) | Out-Null
}
Get-Process codeterm-embed -ErrorAction SilentlyContinue | ForEach-Object {
    Write-Host "[self-install] Stopping codeterm-embed PID $($_.Id) ..."
    $_.Kill()
    $_.WaitForExit(5000) | Out-Null
}

# 2. Run the installer silently. installMode=currentUser in tauri.conf.json
#    means no UAC prompt — `/S` is fully unattended.
Write-Host "[self-install] Running installer ..."
$proc = Start-Process -FilePath $Installer -ArgumentList '/S' -Wait -PassThru
if ($proc.ExitCode -ne 0) {
    Write-Error "[self-install] Installer exited with code $($proc.ExitCode)"
    exit $proc.ExitCode
}
Write-Host "[self-install] Installer finished."

# 3. Verify the new binary is on disk and report its version.
if (-not (Test-Path $expectedExe)) {
    Write-Error "[self-install] Expected $expectedExe after install, not found."
    exit 1
}
$ver = (Get-Item $expectedExe).VersionInfo.FileVersion
Write-Host "[self-install] Installed FileVersion: $ver"

# 4. Launch the new application visibly. A second launch activates the existing
# single-instance window, so this also restores focus after an in-place update.
Write-Host "[self-install] Launching $expectedExe ..."
$app = Start-Process -FilePath $expectedExe -WindowStyle Normal -PassThru
try {
    $null = $app.WaitForInputIdle(30000)
} catch {
    Write-Host "[self-install] Application launch was handed to the existing instance."
}

Write-Host "[self-install] Done."
