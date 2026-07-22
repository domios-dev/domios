$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
$targetDebug = Join-Path $repo ".codeterm-targets\shared\debug"
$namedDataDir = Join-Path $env:USERPROFILE ".codeterm-dev-provider-model-oracle"
$defaultDataDir = Join-Path $env:USERPROFILE ".codeterm-dev"
$nodeModules = Join-Path $repo "node_modules"
$fixturePath = Join-Path $env:TEMP "codeterm-cleanup-selection-$PID.json"

$processes = @(
  @{ ProcessId = 100; ParentProcessId = 1; Name = "codeterm.exe"; ExecutablePath = "$targetDebug\codeterm.exe"; CommandLine = "$targetDebug\codeterm.exe" },
  @{ ProcessId = 110; ParentProcessId = 100; Name = "msedgewebview2.exe"; ExecutablePath = "C:\WebView2\msedgewebview2.exe"; CommandLine = "--user-data-dir=$env:LOCALAPPDATA\com.codeterm.dev\EBWebView" },
  @{ ProcessId = 120; ParentProcessId = 1; Name = "node.exe"; ExecutablePath = "C:\node\node.exe"; CommandLine = "$nodeModules\vite\bin\vite.js --port 1420" },
  @{ ProcessId = 130; ParentProcessId = 100; Name = "msedgewebview2.exe"; ExecutablePath = "C:\WebView2\msedgewebview2.exe"; CommandLine = "--remote-debugging-port=9223" },
  @{ ProcessId = 200; ParentProcessId = 1; Name = "codeterm.exe"; ExecutablePath = "$targetDebug\codeterm.exe"; CommandLine = "$targetDebug\codeterm.exe" },
  @{ ProcessId = 201; ParentProcessId = 200; Name = "child.exe"; ExecutablePath = "C:\fixture\child.exe"; CommandLine = "child-without-direct-ownership" },
  @{ ProcessId = 210; ParentProcessId = 1; Name = "msedgewebview2.exe"; ExecutablePath = "C:\WebView2\msedgewebview2.exe"; CommandLine = "--user-data-dir=$env:LOCALAPPDATA\com.codeterm.dev.provider.model.oracle\EBWebView" },
  @{ ProcessId = 220; ParentProcessId = 1; Name = "node.exe"; ExecutablePath = "C:\node\node.exe"; CommandLine = "$nodeModules\vite\bin\vite.js --port 1560" },
  @{ ProcessId = 230; ParentProcessId = 1; Name = "msedgewebview2.exe"; ExecutablePath = "C:\WebView2\msedgewebview2.exe"; CommandLine = "--remote-debugging-port=9363" },
  @{ ProcessId = 300; ParentProcessId = 1; Name = "codeterm.exe"; ExecutablePath = "$targetDebug\codeterm.exe"; CommandLine = "$targetDebug\codeterm.exe --orphan-no-port" },
  @{ ProcessId = 310; ParentProcessId = 1; Name = "codeterm.exe"; ExecutablePath = "$targetDebug\codeterm.exe"; CommandLine = "$targetDebug\codeterm.exe --wrong-api-port" },
  @{ ProcessId = 320; ParentProcessId = 1; Name = "node.exe"; ExecutablePath = "C:\node\node.exe"; CommandLine = "$nodeModules\vite\bin\vite.js --port 1555" }
  @{ ProcessId = 330; ParentProcessId = 1; Name = "OpenConsole.exe"; ExecutablePath = "$namedDataDir\target\debug\conpty\x64\OpenConsole.exe"; CommandLine = "`"$namedDataDir\target\debug\conpty\x64\OpenConsole.exe`" --headless" }
  @{ ProcessId = 340; ParentProcessId = 1; Name = "OpenConsole.exe"; ExecutablePath = "$defaultDataDir\target\debug\conpty\x64\OpenConsole.exe"; CommandLine = "`"$defaultDataDir\target\debug\conpty\x64\OpenConsole.exe`" --headless" }
)
$listeners = @(
  @{ port = 7686; process_id = 100 },
  @{ port = 1420; process_id = 120 },
  @{ port = 9223; process_id = 130 },
  @{ port = 7787; process_id = 200 },
  @{ port = 1560; process_id = 220 },
  @{ port = 9363; process_id = 230 },
  @{ port = 7799; process_id = 310 },
  @{ port = 1555; process_id = 320 }
)
@{ self_pid = 999; processes = $processes; listeners = $listeners } |
  ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $fixturePath

try {
  $cleanupArgs = @{
    Root = $repo
    TargetDebug = $targetDebug
    DataDir = $namedDataDir
    Identifier = "com.codeterm.dev.provider.model.oracle"
    Instance = "provider-model-oracle"
    ApiPort = 7787
    VitePort = 1560
    CdpPort = 9363
    FixturePath = $fixturePath
    PlanOnly = $true
  }
  $plan = & "$PSScriptRoot\windows-dev-cleanup.ps1" @cleanupArgs | ConvertFrom-Json

  $expected = @(200, 201, 210, 220, 230, 330)
  $actual = @($plan.selected_processes | ForEach-Object { [int]$_ } | Sort-Object)
  if (($actual -join ",") -ne ($expected -join ",")) {
    throw "named cleanup selected [$($actual -join ',')], expected [$($expected -join ',')]"
  }
  foreach ($defaultPid in @(100, 110, 120, 130, 300)) {
    if ($actual -contains $defaultPid) {
      throw "named cleanup selected default/unowned pid $defaultPid"
    }
  }
  if (@($plan.victim_roots) -contains 300) {
    throw "shared executable/target path alone became kill ownership"
  }

  $defaultArgs = @{
    Root = $repo
    TargetDebug = $targetDebug
    DataDir = $defaultDataDir
    Identifier = "com.codeterm.dev"
    ApiPort = 7686
    VitePort = 1420
    CdpPort = 9223
    FixturePath = $fixturePath
    PlanOnly = $true
  }
  $defaultPlan = & "$PSScriptRoot\windows-dev-cleanup.ps1" @defaultArgs | ConvertFrom-Json
  $defaultExpected = @(100, 110, 120, 130, 300, 340)
  $defaultActual = @($defaultPlan.selected_processes | ForEach-Object { [int]$_ } | Sort-Object)
  if (($defaultActual -join ",") -ne ($defaultExpected -join ",")) {
    throw "default cleanup selected [$($defaultActual -join ',')], expected [$($defaultExpected -join ',')]"
  }
  foreach ($wrongOwnerPid in @(200, 201, 210, 220, 230, 310, 320)) {
    if ($defaultActual -contains $wrongOwnerPid) {
      throw "default cleanup selected named/wrong-port pid $wrongOwnerPid"
    }
  }

  $playwrightHelper = Get-Content -Raw -LiteralPath (Join-Path $repo "tests\playwright\helpers\dev-console.ts")
  if ($playwrightHelper -notmatch 'scripts", "windows-dev-cleanup\.ps1"') {
    throw "Playwright cleanup does not call the centralized Windows cleanup script"
  }
  if ($playwrightHelper -match 'Get-CimInstance Win32_Process|function Stop-Tree') {
    throw "Playwright cleanup still contains an inline process selector"
  }
  Write-Host "PASS centralized cleanup preserves named/default ownership and rejects wrong-port processes"
} finally {
  Remove-Item -LiteralPath $fixturePath -Force -ErrorAction SilentlyContinue
}
