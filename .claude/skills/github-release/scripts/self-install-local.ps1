# Install codeterm from a local NSIS installer, replacing the running daemon.
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File scripts\self-install-local.ps1 -Installer <path> [-Resume]
#
# -Installer  Absolute path to CodeTerm_X.Y.Z_x64-setup.exe.
# -Resume     If set, launches codeterm again after install completes.
#             (Auto-resume of tab sessions is a separate concern handled by
#             codeterm's own SessionStart hook; see learning_auto_resume_*.)

param(
    [Parameter(Mandatory=$true)]
    [string]$Installer,

    [switch]$Resume
)

$ErrorActionPreference = 'Stop'

if (-not (Test-Path $Installer)) {
    Write-Error "Installer not found: $Installer"
    exit 1
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
}

# Brief settle so NSIS sees no file locks.
Start-Sleep -Milliseconds 500

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

# 4. Launch the new daemon if asked. Detached so this script can return.
if ($Resume) {
    Write-Host "[self-install] Launching $expectedExe ..."
    Start-Process -FilePath $expectedExe -WindowStyle Hidden | Out-Null
}

Write-Host "[self-install] Done."
