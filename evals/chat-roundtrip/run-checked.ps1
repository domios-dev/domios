# WHY: PowerShell does not propagate a native pipeline's exit status automatically.
[CmdletBinding()]
param(
    [string]$LogPath = "target-check/evals/chat-roundtrip/phase-b.log",
    [ValidateRange(-1, [int]::MaxValue)]
    [int]$VerdictOnly = -1
)

$ErrorActionPreference = "Stop"
$candidates = @(
    "$env:ProgramFiles\Git\bin\bash.exe",
    "$env:ProgramFiles\Git\usr\bin\bash.exe",
    "${env:ProgramFiles(x86)}\Git\bin\bash.exe",
    "C:\msys64\usr\bin\bash.exe"
) | Where-Object { $_ -and (Test-Path -LiteralPath $_) }

if (-not $candidates -or $candidates.Count -eq 0) {
    throw "Git Bash/MSYS bash not found. Install Git for Windows or run run.sh from a real bash."
}

$bash = $candidates[0]
$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "..\..")).Path
Set-Location $repoRoot

$logParent = Split-Path -Parent $LogPath
if ($logParent) {
    New-Item -ItemType Directory -Force $logParent | Out-Null
}

if ($VerdictOnly -ge 0) {
    & $bash -lc 'set -o pipefail; source evals/chat-roundtrip/run.sh --verdict-only "$1" 2>&1 | tee "$2"' _ "$VerdictOnly" "$LogPath"
} else {
    & $bash -lc 'set -o pipefail; source evals/chat-roundtrip/run.sh 2>&1 | tee "$1"' _ "$LogPath"
}
exit $LASTEXITCODE
