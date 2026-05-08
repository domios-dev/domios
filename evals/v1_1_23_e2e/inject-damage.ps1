#Requires -Version 5.1
<#
.SYNOPSIS
  v1.1.23 E2E — inject deliberate damage into the dev mem DB.

.DESCRIPTION
  Picks 3-5 active relations and 1-2 entities to delete in the dev DB.
  Records the damage manifest at evals/v1_1_23_e2e/damage-manifest.md so
  every step of E2E knows what should heal.

  THIS IS THE ONLY ALLOWED RAW-SQL BYPASS in v1.1.23 — explicitly outside
  the store layer because we are *creating* the bad state on purpose.

  Implementation note: the actual sqlite3 calls live in inject_damage.py
  (PowerShell has no built-in sqlite3, but Python 3.12 ships with one and
  is on PATH on BLACKSTAR). This wrapper just dispatches to Python.

.PARAMETER DbPath
  Path to the dev mem DB. Refuses any path not under ~/.codeterm-dev/.

.PARAMETER ManifestPath
  Where to write the damage manifest.
#>

param(
    [string]$DbPath = (Join-Path $HOME ".codeterm-dev/mem/knowledge.db"),
    [string]$ManifestPath = "evals/v1_1_23_e2e/damage-manifest.md",
    [int]$Rels = 4,
    [int]$Ents = 2
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (-not $repoRoot) {
    $repoRoot = (Get-Location).Path
}

$pyScript = Join-Path $PSScriptRoot "inject_damage.py"
if (-not (Test-Path $pyScript)) {
    throw "Missing $pyScript"
}

Push-Location $repoRoot
try {
    & python $pyScript --db "$DbPath" --manifest "$ManifestPath" --rels $Rels --ents $Ents
    if ($LASTEXITCODE -ne 0) {
        throw "inject_damage.py exited with code $LASTEXITCODE"
    }
} finally {
    Pop-Location
}
