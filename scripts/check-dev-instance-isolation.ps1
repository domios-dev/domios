param(
  [int] $NamedPort = 7787,
  [string] $NamedInstance = "provider-model-oracle"
)

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot

function ListenerPid([int] $Port) {
  $listener = Get-NetTCPConnection -State Listen -LocalPort $Port -ErrorAction SilentlyContinue |
    Select-Object -First 1
  if ($listener) { return [int]$listener.OwningProcess }
  return $null
}

function Health([int] $Port) {
  try {
    Invoke-RestMethod "http://127.0.0.1:$Port/api/health" -TimeoutSec 3
  } catch {
    return $null
  }
}

function Assert-DefaultHealth($Health, [string] $ExpectedSha) {
  if (-not $Health -or $Health.status -ne "ok") {
    throw "default dev 7686 must be healthy before and after the named cleanup oracle"
  }
  if ($Health.build_sha -ne $ExpectedSha) {
    throw "default dev provenance mismatch: expected $ExpectedSha, got $($Health.build_sha)"
  }
  if ($Health.instance.name -ne "" -or [string]$Health.instance.api_port -ne "7686" -or
      [string]$Health.instance.vite_port -ne "1420" -or [string]$Health.instance.cdp_port -ne "9223" -or
      $Health.instance.identifier -ne "com.codeterm.dev") {
    throw "default dev instance identity is stale or mismatched"
  }
}

Push-Location $repo
try {
  $expectedSha = (& git rev-parse HEAD).Trim()
  $prodBefore = ListenerPid 7685
  $defaultBefore = ListenerPid 7686
  if (-not $defaultBefore) {
    throw "warm integration dev must already own port 7686"
  }
  if (ListenerPid $NamedPort) {
    throw "named oracle port $NamedPort must be unused before cleanup"
  }
  Assert-DefaultHealth (Health 7686) $expectedSha

  $savedInstance = $env:CODETERM_INSTANCE
  $savedApiPort = $env:CODETERM_API_PORT
  try {
    $env:CODETERM_INSTANCE = $NamedInstance
    $env:CODETERM_API_PORT = [string]$NamedPort
    & .\build.ps1 kill
    if ($LASTEXITCODE -ne 0) {
      throw "named cleanup exited with $LASTEXITCODE"
    }
  } finally {
    $env:CODETERM_INSTANCE = $savedInstance
    $env:CODETERM_API_PORT = $savedApiPort
  }

  $defaultAfter = ListenerPid 7686
  if ($defaultAfter -ne $defaultBefore) {
    throw "named cleanup changed default 7686 pid: before=$defaultBefore after=$defaultAfter"
  }
  if ((ListenerPid 7685) -ne $prodBefore) {
    throw "named cleanup changed production 7685 pid"
  }
  if (ListenerPid $NamedPort) {
    throw "named cleanup left a listener on $NamedPort"
  }
  Assert-DefaultHealth (Health 7686) $expectedSha
  Write-Host "PASS named cleanup preserved default pid=$defaultBefore and production pid=$prodBefore"
} finally {
  Pop-Location
}
