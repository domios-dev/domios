# Caller: scripts check-build-kill-safety.sh
$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
$targetDebug = Join-Path $repo "src-tauri\target\debug"
$defaultTargetDebug = Join-Path $repo ".codeterm-targets\default\debug"
$namedDataDir = Join-Path $env:USERPROFILE ".domios-dev-provider-model-oracle"
$defaultDataDir = Join-Path $env:USERPROFILE ".domios-dev"
$nodeModules = Join-Path $repo "node_modules"
$fixturePath = Join-Path $env:TEMP "codeterm-cleanup-selection-$PID.json"
$ownedRecordPaths = @()

$processes = @(
  @{ ProcessId = 100; ParentProcessId = 1; Name = "codeterm.exe"; ExecutablePath = "$targetDebug\codeterm.exe"; CommandLine = "$targetDebug\codeterm.exe" },
  @{ ProcessId = 110; ParentProcessId = 100; Name = "msedgewebview2.exe"; ExecutablePath = "C:\WebView2\msedgewebview2.exe"; CommandLine = "--user-data-dir=$env:LOCALAPPDATA\com.domios.dev\EBWebView" },
  @{ ProcessId = 120; ParentProcessId = 1; Name = "node.exe"; ExecutablePath = "C:\node\node.exe"; CommandLine = "$nodeModules\vite\bin\vite.js --port 1420" },
  @{ ProcessId = 130; ParentProcessId = 100; Name = "msedgewebview2.exe"; ExecutablePath = "C:\WebView2\msedgewebview2.exe"; CommandLine = "--remote-debugging-port=9223 --user-data-dir=$env:LOCALAPPDATA\com.domios.dev\EBWebView" },
  @{ ProcessId = 200; ParentProcessId = 1; Name = "codeterm.exe"; ExecutablePath = "$targetDebug\codeterm.exe"; CommandLine = "$targetDebug\codeterm.exe" },
  @{ ProcessId = 201; ParentProcessId = 200; Name = "child.exe"; ExecutablePath = "C:\fixture\child.exe"; CommandLine = "child-without-direct-ownership" },
  @{ ProcessId = 210; ParentProcessId = 1; Name = "msedgewebview2.exe"; ExecutablePath = "C:\WebView2\msedgewebview2.exe"; CommandLine = "--user-data-dir=$env:LOCALAPPDATA\com.domios.dev.provider.model.oracle\EBWebView" },
  @{ ProcessId = 220; ParentProcessId = 1; Name = "node.exe"; ExecutablePath = "C:\node\node.exe"; CommandLine = "$nodeModules\vite\bin\vite.js --port 1560" },
  @{ ProcessId = 225; ParentProcessId = 1; Name = "node.exe"; ExecutablePath = "C:\node\node.exe"; CommandLine = "$nodeModules\vite\bin\vite.js build --watch" },
  @{ ProcessId = 240; ParentProcessId = 1; Name = "node.exe"; ExecutablePath = "C:\node\node.exe"; CommandLine = "node node_modules/vite/bin/vite.js build --watch" },
  @{ ProcessId = 241; ParentProcessId = 240; Name = "node.exe"; ExecutablePath = "C:\node\node.exe"; CommandLine = "vite watcher child" },
  @{ ProcessId = 250; ParentProcessId = 1; Name = "node.exe"; ExecutablePath = "C:\node\node.exe"; CommandLine = "node scripts/dev-static-server.mjs" },
  @{ ProcessId = 251; ParentProcessId = 250; Name = "node.exe"; ExecutablePath = "C:\node\node.exe"; CommandLine = "static server child" },
  @{ ProcessId = 230; ParentProcessId = 1; Name = "msedgewebview2.exe"; ExecutablePath = "C:\WebView2\msedgewebview2.exe"; CommandLine = "--remote-debugging-port=9363 --user-data-dir=$env:LOCALAPPDATA\com.domios.dev.provider.model.oracle\EBWebView" },
  @{ ProcessId = 300; ParentProcessId = 1; Name = "codeterm.exe"; ExecutablePath = "$defaultTargetDebug\codeterm.exe"; CommandLine = "$defaultTargetDebug\codeterm.exe --orphan-no-port" },
  @{ ProcessId = 310; ParentProcessId = 1; Name = "codeterm.exe"; ExecutablePath = "$targetDebug\codeterm.exe"; CommandLine = "$targetDebug\codeterm.exe --wrong-api-port" },
  @{ ProcessId = 320; ParentProcessId = 1; Name = "node.exe"; ExecutablePath = "C:\node\node.exe"; CommandLine = "$nodeModules\vite\bin\vite.js --port 1555" }
  @{ ProcessId = 330; ParentProcessId = 9998; Name = "OpenConsole.exe"; ExecutablePath = "$targetDebug\conpty\x64\OpenConsole.exe"; CommandLine = "`"$targetDebug\conpty\x64\OpenConsole.exe`" --headless" }
  @{ ProcessId = 336; ParentProcessId = 9994; Name = "OpenConsole.exe"; ExecutablePath = "\\?\$targetDebug\conpty\x64\OpenConsole.exe"; CommandLine = "OpenConsole.exe --headless" }
  @{ ProcessId = 331; ParentProcessId = 200; Name = "OpenConsole.exe"; ExecutablePath = "$targetDebug\conpty\x64\OpenConsole.exe"; CommandLine = "`"$targetDebug\conpty\x64\OpenConsole.exe`" --headless" }
  @{ ProcessId = 332; ParentProcessId = 500; Name = "OpenConsole.exe"; ExecutablePath = "$targetDebug\conpty\x64\OpenConsole.exe"; CommandLine = "`"$targetDebug\conpty\x64\OpenConsole.exe`" --headless" }
  @{ ProcessId = 333; ParentProcessId = 9997; Name = "OpenConsole.exe"; ExecutablePath = "$namedDataDir\target\debug\conpty\x64\OpenConsole.exe"; CommandLine = "`"$namedDataDir\target\debug\conpty\x64\OpenConsole.exe`" --headless" }
  @{ ProcessId = 334; ParentProcessId = 9996; Name = "OpenConsole.exe"; ExecutablePath = "C:\Program Files\CodeTerm\conpty\x64\OpenConsole.exe"; CommandLine = "`"C:\Program Files\CodeTerm\conpty\x64\OpenConsole.exe`" --headless" }
  @{ ProcessId = 340; ParentProcessId = 9995; Name = "OpenConsole.exe"; ExecutablePath = "$defaultTargetDebug\conpty\x64\OpenConsole.exe"; CommandLine = "`"$defaultTargetDebug\conpty\x64\OpenConsole.exe`" --headless" }
  @{ ProcessId = 350; ParentProcessId = 1; Name = "fixture.exe"; ExecutablePath = "$defaultTargetDebug\deps\fixture.exe"; CommandLine = "$defaultTargetDebug\deps\fixture.exe" }
  @{ ProcessId = 351; ParentProcessId = 1; Name = "build-script-build.exe"; ExecutablePath = "$defaultTargetDebug\build\fixture\build-script-build.exe"; CommandLine = "$defaultTargetDebug\build\fixture\build-script-build.exe" }
  @{ ProcessId = 500; ParentProcessId = 1; Name = "foreign-daemon.exe"; ExecutablePath = "C:\other\daemon.exe"; CommandLine = "foreign daemon" }
  @{ ProcessId = 550; ParentProcessId = 1; Name = "codeterm.exe"; ExecutablePath = "$targetDebug\codeterm.exe"; CommandLine = "$targetDebug\codeterm.exe" }
  @{ ProcessId = 335; ParentProcessId = 550; Name = "OpenConsole.exe"; ExecutablePath = "$targetDebug\conpty\x64\OpenConsole.exe"; CommandLine = "`"$targetDebug\conpty\x64\OpenConsole.exe`" --headless" }
)
$listeners = @(
  @{ port = 7686; process_id = 100 },
  @{ port = 1420; process_id = 120 },
  @{ port = 9223; process_id = 130 },
  @{ port = 7787; process_id = 200 },
  @{ port = 1560; process_id = 220 },
  @{ port = 9363; process_id = 230 },
  @{ port = 7799; process_id = 310 },
  @{ port = 7788; process_id = 550 },
  @{ port = 1555; process_id = 320 }
)
foreach ($process in $processes) { $process.created = [string]([long]$process.ProcessId * 100) }
@{ self_pid = 999; processes = $processes; listeners = $listeners } |
  ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $fixturePath

try {
  $cleanupArgs = @{
    Root = $repo
    TargetDebug = $targetDebug
    DataDir = $namedDataDir
    Identifier = "com.domios.dev.provider.model.oracle"
    Instance = "provider-model-oracle"
    ApiPort = 7787
    VitePort = 1560
    CdpPort = 9363
    FixturePath = $fixturePath
    PlanOnly = $true
  }
  $plan = & "$PSScriptRoot\windows-dev-cleanup.ps1" @cleanupArgs | ConvertFrom-Json

  $expected = @(200, 201, 210, 220, 230, 330, 331, 333, 336)
  $actual = @($plan.selected_processes | ForEach-Object { [int]$_ } | Sort-Object)
  if (($actual -join ",") -ne ($expected -join ",")) {
    throw "named cleanup selected [$($actual -join ',')], expected [$($expected -join ',')]"
  }
  foreach ($defaultPid in @(100, 110, 120, 130, 225, 300, 332, 334, 335, 550)) {
    if ($actual -contains $defaultPid) {
      throw "named cleanup selected default/unowned pid $defaultPid"
    }
  }
  if (@($plan.victim_roots) -contains 300) {
    throw "shared executable/target path alone became kill ownership"
  }

  $ownedArgs = $cleanupArgs.Clone()
  foreach ($procId in @(240, 250)) {
    $recordPath = "$fixturePath.$procId.json"
    $process = $processes | Where-Object { $_.ProcessId -eq $procId }
    @{ pid = $procId; created = $process.created; executable = $process.ExecutablePath } |
      ConvertTo-Json | Set-Content -LiteralPath $recordPath
    $ownedRecordPaths += $recordPath
  }
  $ownedArgs.OwnedProcessRecords = $ownedRecordPaths
  $ownedArgs.OwnedOnly = $true
  $ownedPlan = & "$PSScriptRoot\windows-dev-cleanup.ps1" @ownedArgs | ConvertFrom-Json
  $ownedExpected = @(240, 241, 250, 251)
  $ownedActual = @($ownedPlan.selected_processes | ForEach-Object { [int]$_ } | Sort-Object)
  if (($ownedActual -join ",") -ne ($ownedExpected -join ",")) {
    throw "owned cleanup selected [$($ownedActual -join ',')], expected [$($ownedExpected -join ',')]"
  }
  if (@($ownedPlan.selected_processes) -contains 225) {
    throw "owned cleanup selected foreign watcher 225"
  }

  $oldChild = $processes | Where-Object { $_.ProcessId -eq 241 }
  $oldChild.created = '23900'
  @{ self_pid = 999; processes = $processes; listeners = $listeners } |
    ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $fixturePath
  $oldChildPlan = & "$PSScriptRoot\windows-dev-cleanup.ps1" @ownedArgs 3>$null | ConvertFrom-Json
  if (@($oldChildPlan.selected_processes) -contains 241) { throw 'reused parent PID selected an older foreign child' }
  $oldChild.created = '24100'
  @{ self_pid = 999; processes = $processes; listeners = $listeners } |
    ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $fixturePath

  foreach ($mismatch in @('created', 'executable')) {
    $record = Get-Content -Raw -LiteralPath $ownedRecordPaths[0] | ConvertFrom-Json
    $record.$mismatch = if ($mismatch -eq 'created') { '1' } else { 'C:\foreign\node.exe' }
    $record | ConvertTo-Json | Set-Content -LiteralPath $ownedRecordPaths[0]
    $mismatchArgs = $ownedArgs.Clone()
    $mismatchArgs.OwnedProcessRecords = @($ownedRecordPaths[0])
    $mismatchPlan = & "$PSScriptRoot\windows-dev-cleanup.ps1" @mismatchArgs 3>$null | ConvertFrom-Json
    if (@($mismatchPlan.selected_processes).Count -ne 0 -or @($mismatchPlan.skipped).Count -ne 1) {
      throw "reused PID with $mismatch mismatch selected a foreign process/tree"
    }
    $process = $processes | Where-Object { $_.ProcessId -eq 240 }
    @{ pid = 240; created = $process.created; executable = $process.ExecutablePath } |
      ConvertTo-Json | Set-Content -LiteralPath $ownedRecordPaths[0]
  }
  '240' | Set-Content -LiteralPath $ownedRecordPaths[0]
  $invalidArgs = $ownedArgs.Clone()
  $invalidArgs.OwnedProcessRecords = @($ownedRecordPaths[0])
  $invalidPlan = & "$PSScriptRoot\windows-dev-cleanup.ps1" @invalidArgs 3>$null | ConvertFrom-Json
  if (@($invalidPlan.selected_processes).Count -ne 0) { throw 'bare PID became kill ownership' }

  $defaultArgs = @{
    Root = $repo
    TargetDebug = $defaultTargetDebug
    DataDir = $defaultDataDir
    Identifier = "com.domios.dev"
    ApiPort = 7686
    VitePort = 1420
    CdpPort = 9223
    FixturePath = $fixturePath
    PlanOnly = $true
  }
  $defaultPlan = & "$PSScriptRoot\windows-dev-cleanup.ps1" @defaultArgs | ConvertFrom-Json
  $defaultExpected = @(110, 120, 130, 300, 340)
  $defaultActual = @($defaultPlan.selected_processes | ForEach-Object { [int]$_ } | Sort-Object)
  if (($defaultActual -join ",") -ne ($defaultExpected -join ",")) {
    throw "default cleanup selected [$($defaultActual -join ',')], expected [$($defaultExpected -join ',')]"
  }
  foreach ($wrongOwnerPid in @(100, 200, 201, 210, 220, 225, 230, 240, 241, 250, 251, 310, 320, 330, 331, 332, 333, 334, 350, 351)) {
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
  Write-Host "PASS centralized cleanup preserves named/default ownership and stops only explicit watcher trees"
} finally {
  foreach ($recordPath in $ownedRecordPaths) { Remove-Item -LiteralPath $recordPath -Force -ErrorAction SilentlyContinue }
  Remove-Item -LiteralPath $fixturePath -Force -ErrorAction SilentlyContinue
}
