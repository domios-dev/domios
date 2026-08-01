# WHY: PowerShell does not propagate a native pipeline's exit status automatically.
[CmdletBinding()]
param(
    [string]$LogPath = "target-check/evals/chat-roundtrip/phase-b.log",
    [ValidateRange(-1, [int]::MaxValue)]
    [int]$VerdictOnly = -1
)

$ErrorActionPreference = "Stop"
$repoRoot = git rev-parse --show-toplevel
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}
Set-Location $repoRoot

$logParent = Split-Path -Parent $LogPath
if ($logParent) {
    New-Item -ItemType Directory -Force $logParent | Out-Null
}

if ($VerdictOnly -ge 0) {
    & bash -lc 'set -o pipefail; bash evals/chat-roundtrip/run.sh --verdict-only "$1" 2>&1 | tee "$2"' _ "$VerdictOnly" "$LogPath"
} else {
    & bash -lc 'set -o pipefail; bash evals/chat-roundtrip/run.sh 2>&1 | tee "$1"' _ "$LogPath"
}
exit $LASTEXITCODE
