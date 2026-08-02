[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$LanePath,
    [string]$Branch,
    [string]$TabId,
    [string]$PlanId,
    [string]$TaskId,
    [string]$CanonicalPath = (Get-Location).Path,
    [string]$WorktreeRoot = 'D:\Developer\codeterm-worktrees',
    [switch]$AllowUnregisteredResidue
)

$ErrorActionPreference = 'Stop'

function Resolve-ExistingPath([string]$Path) {
    (Resolve-Path -LiteralPath $Path).Path.TrimEnd('\')
}

function Assert-Success([string]$Operation) {
    if ($LASTEXITCODE -ne 0) {
        throw "$Operation failed with exit code $LASTEXITCODE"
    }
}

$canonical = Resolve-ExistingPath $CanonicalPath
$root = Resolve-ExistingPath $WorktreeRoot
$lane = Resolve-ExistingPath $LanePath

if ($lane -eq $canonical -or -not $lane.StartsWith($root + '\', [StringComparison]::OrdinalIgnoreCase)) {
    throw "Refusing lane outside worktree root: $lane"
}

if ($TabId) {
    codeterm tab close $TabId
    Assert-Success "Close CodeTerm tab $TabId"
}

$gitMarker = Join-Path $lane '.git'
if (Test-Path -LiteralPath $gitMarker) {
    $dirty = @(git -C $lane status --porcelain)
    Assert-Success 'Read lane status'
    if ($dirty.Count -ne 0) {
        throw "Refusing dirty lane: $lane"
    }

    $laneHead = (git -C $lane rev-parse HEAD).Trim()
    Assert-Success 'Read lane HEAD'
    git -C $canonical merge-base --is-ancestor $laneHead HEAD
    Assert-Success "Verify lane ancestry for $laneHead"

    git -C $canonical worktree remove $lane
    Assert-Success "Remove registered worktree $lane"
} elseif (-not $AllowUnregisteredResidue) {
    throw "Refusing unregistered residue without -AllowUnregisteredResidue: $lane"
}

if (Test-Path -LiteralPath $lane) {
    $reserved = Get-ChildItem -LiteralPath $lane -Force -Recurse -ErrorAction Stop |
        Where-Object { $_.Name -match '^(?i:CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\..*)?$' } |
        Sort-Object { $_.FullName.Length } -Descending

    foreach ($artifact in $reserved) {
        $extendedPath = '\\?\' + $artifact.FullName
        Remove-Item -LiteralPath $extendedPath -Force -Recurse
    }

    $remaining = @(Get-ChildItem -LiteralPath $lane -Force)
    if ($remaining.Count -eq 0) {
        Remove-Item -LiteralPath $lane
    } else {
        Remove-Item -LiteralPath $lane -Force -Recurse
    }
}

if (Test-Path -LiteralPath $lane) {
    throw "Lane residue still exists: $lane"
}

if ($Branch) {
    git -C $canonical branch -d $Branch
    Assert-Success "Delete merged branch $Branch"
}

if ($PlanId -and $TaskId) {
    codeterm plan task update $PlanId $TaskId --cleanup-status done
    Assert-Success "Record cleanup for $PlanId/$TaskId"
}

Write-Output "CLEANED $lane"
