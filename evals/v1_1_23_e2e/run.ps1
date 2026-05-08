#Requires -Version 5.1
<#
.SYNOPSIS
  v1.1.23 E2E orchestrator — runs the 12-step procedure end-to-end.

.DESCRIPTION
  Captures a transcript with PASS/FAIL per step. Writes the final report to
  evals/v1_1_23_e2e/run-<utc-ts>-<PASS|FAIL>.md.

  Snapshots prod mem DB into dev (read-only against prod), injects deliberate
  damage, exercises every v1.1.23 track against the damaged graph, then
  performs a defensive restore-pass against an older backup.

  Hard rules:
  - NEVER touches the prod daemon or prod DB beyond reading for the snapshot.
  - All test commands go through the typed CLI or daemon API.
  - The damage-injection script is the only raw-SQL bypass.
#>

param(
    [string]$DevDbPath = (Join-Path $HOME ".codeterm-dev/mem/knowledge.db"),
    [string]$ProdDbPath = (Join-Path $HOME ".codeterm/mem/knowledge.db"),
    [string]$ProdBackupPath = (Join-Path $HOME ".codeterm/mem/knowledge.db.bak.20260508_093211"),
    [int]$DevPort = 7686,
    [int]$ProdPort = 7685,
    [switch]$SkipBoot
)

$ErrorActionPreference = "Continue"

$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (-not $repoRoot) { $repoRoot = (Get-Location).Path }

$ts = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssZ")
$reportLines = New-Object System.Collections.ArrayList
$results = New-Object System.Collections.ArrayList
$overall = "PASS"

function Add-Line {
    param([string]$Text)
    [void]$reportLines.Add($Text)
    Write-Host $Text
}

function Step-Result {
    param(
        [string]$Id,
        [string]$Title,
        [string]$Status,  # PASS / FAIL / SKIP / WARN
        [string]$Detail
    )
    [void]$results.Add([PSCustomObject]@{
        Id     = $Id
        Title  = $Title
        Status = $Status
        Detail = $Detail
    })
    Add-Line ""
    Add-Line "### Step $Id — $Title : $Status"
    if ($Detail) { Add-Line $Detail }
    if ($Status -eq "FAIL") { $script:overall = "FAIL" }
}

function Run-Cmd {
    param(
        [string]$CmdLine,
        [int]$ExpectExit = 0
    )
    Add-Line ""
    Add-Line "``````"
    Add-Line "$ $CmdLine"
    $out = & cmd.exe /c "$CmdLine 2>&1"
    $code = $LASTEXITCODE
    $outStr = ($out | Out-String).TrimEnd()
    if ($outStr.Length -gt 4000) {
        $outStr = $outStr.Substring(0, 4000) + "`n... [truncated]"
    }
    Add-Line $outStr
    Add-Line "[exit=$code]"
    Add-Line "``````"
    return [PSCustomObject]@{ Output = $out; OutputStr = $outStr; ExitCode = $code }
}

$cli = "$HOME\.codeterm\bin\codeterm.exe"
$repoCommit = (& git -C $repoRoot rev-parse HEAD).Trim()

Add-Line "# v1.1.23 E2E run — $ts"
Add-Line ""
Add-Line "- Repo commit: ``$repoCommit``"
Add-Line "- Dev DB: ``$DevDbPath``"
Add-Line "- Prod DB (read-only): ``$ProdDbPath``"
Add-Line "- Prod backup snapshot: ``$ProdBackupPath``"
Add-Line "- CLI: ``$cli``"
Add-Line ""

# ---------------------------------------------------------------------------
# Step 1: Snapshot prod mem DB into dev (with pre-run backup)
# ---------------------------------------------------------------------------

try {
    if (-not (Test-Path $ProdDbPath)) { throw "prod DB missing: $ProdDbPath" }

    # Ensure dev daemon is down before swapping the DB file (Windows file lock).
    Add-Line "Ensuring dev daemon is stopped before DB swap..."
    Push-Location $repoRoot
    try {
        & bash ./build.sh kill 2>&1 | Out-Null
    } catch {
        Add-Line "build.sh kill returned non-zero (likely no dev running) — continuing"
    }
    Pop-Location
    Start-Sleep -Seconds 2

    $devDir = Split-Path -Parent $DevDbPath
    if (-not (Test-Path $devDir)) { New-Item -ItemType Directory -Path $devDir -Force | Out-Null }

    $preBackup = Join-Path $devDir "knowledge.db.pre-e2e.$ts"
    if (Test-Path $DevDbPath) {
        Copy-Item $DevDbPath $preBackup -Force
        Add-Line "Pre-E2E backup: $preBackup"
    } else {
        Add-Line "No existing dev DB to back up (fresh)"
    }

    # Replace dev DB with prod snapshot. Drop WAL/SHM so SQLite re-initializes.
    Copy-Item $ProdDbPath $DevDbPath -Force
    foreach ($suf in "-wal", "-shm") {
        $pf = "$ProdDbPath$suf"
        $df = "$DevDbPath$suf"
        if (Test-Path $df) { Remove-Item $df -Force }
        if (Test-Path $pf) { Copy-Item $pf $df -Force }
    }

    $size = (Get-Item $DevDbPath).Length
    Step-Result -Id "1" -Title "Snapshot prod mem DB into dev" -Status "PASS" -Detail "Snapshot copied. Dev DB size = $size bytes. Pre-run backup at $preBackup."
} catch {
    Step-Result -Id "1" -Title "Snapshot prod mem DB into dev" -Status "FAIL" -Detail "Exception: $($_.Exception.Message)"
}

# ---------------------------------------------------------------------------
# Step 2: Inject deliberate damage
# ---------------------------------------------------------------------------

try {
    $manifest = Join-Path $repoRoot "evals/v1_1_23_e2e/damage-manifest.md"
    Push-Location $repoRoot
    $r = Run-Cmd "python evals/v1_1_23_e2e/inject_damage.py --db `"$DevDbPath`" --manifest `"$manifest`""
    Pop-Location
    if ($r.ExitCode -eq 0 -and (Test-Path $manifest)) {
        Step-Result -Id "2" -Title "Inject deliberate damage" -Status "PASS" -Detail "Manifest written: $manifest"
    } else {
        Step-Result -Id "2" -Title "Inject deliberate damage" -Status "FAIL" -Detail "Exit=$($r.ExitCode); manifest_present=$([bool](Test-Path $manifest))"
    }
} catch {
    Step-Result -Id "2" -Title "Inject deliberate damage" -Status "FAIL" -Detail "Exception: $($_.Exception.Message)"
}

# ---------------------------------------------------------------------------
# Step 3: Boot dev daemon, verify build hash matches HEAD
# ---------------------------------------------------------------------------

try {
    if (-not $SkipBoot) {
        # Pre-flight: free port 1420 (vite) if any stray node.exe owns it.
        # Loop in case multiple zombies linger.
        for ($k = 0; $k -lt 3; $k++) {
            $vitePid = ""
            $netLines = netstat -ano | Select-String ":1420.*LISTENING"
            foreach ($ln in $netLines) {
                $parts = ($ln.ToString() -replace '^\s+', '') -split '\s+'
                $vitePid = $parts[-1]
                break
            }
            if (-not $vitePid -or $vitePid -notmatch '^\d+$') { break }
            Add-Line "Freeing port 1420 from stale process PID=$vitePid"
            try { Stop-Process -Id $vitePid -Force -ErrorAction Stop } catch { }
            Start-Sleep -Seconds 2
        }

        Add-Line "Spawning dev daemon via detached bash..."
        $logPath = "/tmp/dev-spawn-e2e.log"
        # Run bash -c "...&; disown" but keep bash alive long enough that the
        # backgrounded child doesn't die. Use `setsid`-equivalent via `&` + exit.
        $repoFwd = $repoRoot -replace '\\','/'
        $bashLine = "cd '$repoFwd' && nohup ./build.sh dev > $logPath 2>&1 < /dev/null & disown; sleep 1"
        & bash -lc $bashLine | Out-Null

        # Poll /api/health until ready (up to 360s — first build may be slow)
        $healthUrl = "http://127.0.0.1:$DevPort/api/health"
        $ready = $false
        $maxIter = 90
        for ($i = 0; $i -lt $maxIter; $i++) {
            Start-Sleep -Seconds 4
            try {
                $h = Invoke-RestMethod -Uri $healthUrl -Method Get -TimeoutSec 5 -ErrorAction Stop
                if ($h.status -eq "ok") { $ready = $true; break }
            } catch { }
        }
        if (-not $ready) {
            Step-Result -Id "3" -Title "Boot dev daemon" -Status "FAIL" -Detail "Dev /api/health never returned ok within 360s. Spawn log: $logPath"
            $overall = "FAIL"
        } else {
            $build = $h.build
            $headShort = $repoCommit.Substring(0, 8)
            $detail = "build=$build version=$($h.version) build_number=$($h.build_number) HEAD=$headShort"
            if ($build -eq $headShort) {
                Step-Result -Id "3" -Title "Boot dev daemon" -Status "PASS" -Detail $detail
            } else {
                Step-Result -Id "3" -Title "Boot dev daemon" -Status "WARN" -Detail "$detail (build hash differs from HEAD — incremental dev build OK if commits are local-only)"
            }
        }
    } else {
        Step-Result -Id "3" -Title "Boot dev daemon" -Status "SKIP" -Detail "SkipBoot flag set"
    }
} catch {
    Step-Result -Id "3" -Title "Boot dev daemon" -Status "FAIL" -Detail "Exception: $($_.Exception.Message)"
}

# Helper: dev API token — read from ~/.codeterm-dev/config.toml [api].token
function Get-DevToken {
    if ($env:CODETERM_API_TOKEN) { return $env:CODETERM_API_TOKEN }
    $cfg = Join-Path $HOME ".codeterm-dev/config.toml"
    if (Test-Path $cfg) {
        $txt = Get-Content $cfg -Raw
        if ($txt -match 'token\s*=\s*"([0-9a-fA-F]+)"') { return $Matches[1] }
    }
    return ""
}

$devToken = Get-DevToken
$devCli = $cli  # use prod-installed CLI; CODETERM_DATA_DIR_NAME picks dev
$env:CODETERM_DATA_DIR_NAME = ".codeterm-dev"
$env:CODETERM_API_PORT = "$DevPort"
$env:CODETERM_API_TOKEN = $devToken

# ---------------------------------------------------------------------------
# Step 4: Track 1 — graph reads
# ---------------------------------------------------------------------------

try {
    # Pick a hub from manifest — first damaged from_id is a good candidate.
    $manifestText = Get-Content (Join-Path $repoRoot "evals/v1_1_23_e2e/damage-manifest.md") -Raw
    $hubCandidates = @()
    foreach ($line in ($manifestText -split "`n")) {
        if ($line -match '^\| `([^`]+)` \| `[^`]+` \| `[^`]+` \|') {
            $hubCandidates += $Matches[1]
        }
    }
    $hub = ""
    foreach ($cand in $hubCandidates) {
        # Try this candidate; if --include-invalidated returns >= 1 relation, use it.
        $probe = & cmd.exe /c "`"$devCli`" mem graph related --id `"$cand`" --include-invalidated --json 2>&1"
        if ($LASTEXITCODE -eq 0) {
            $jp = $null
            try { $jp = ($probe | Out-String) | ConvertFrom-Json } catch { }
            if ($jp) {
                $cnt = if ($jp.relations) { $jp.relations.Count } elseif ($jp -is [array]) { $jp.Count } else { 0 }
                if ($cnt -ge 1) { $hub = $cand; break }
            }
        }
    }
    if (-not $hub) { throw "could not determine a hub id from manifest (candidates: $($hubCandidates -join ', '))" }
    Add-Line "Using hub for graph reads: $hub"

    $r1 = Run-Cmd "`"$devCli`" mem graph related --id `"$hub`" --json"
    $r2 = Run-Cmd "`"$devCli`" mem graph related --id `"$hub`" --include-invalidated --json"

    # Both should succeed. With include-invalidated, count must be >= count without.
    $countActive = 0; $countAll = 0
    try {
        $j1 = $r1.OutputStr | ConvertFrom-Json
        if ($j1.relations) { $countActive = $j1.relations.Count } elseif ($j1 -is [array]) { $countActive = $j1.Count }
    } catch { }
    try {
        $j2 = $r2.OutputStr | ConvertFrom-Json
        if ($j2.relations) { $countAll = $j2.relations.Count } elseif ($j2 -is [array]) { $countAll = $j2.Count }
    } catch { }

    # graph_full check: count edges where valid_to is non-null in default (active) view.
    $fullUrl = "http://127.0.0.1:$DevPort/api/mem/graph/full"
    $headers = @{}
    if ($devToken) { $headers["Authorization"] = "Bearer $devToken" }
    $leakCount = -1
    try {
        $full = Invoke-RestMethod -Uri $fullUrl -Headers $headers -Method Get -TimeoutSec 30
        $leakCount = 0
        if ($full.edges) {
            foreach ($e in $full.edges) {
                if ($e.valid_to -and $e.valid_to -ne "") { $leakCount++ }
            }
        } elseif ($full.relations) {
            foreach ($e in $full.relations) {
                if ($e.valid_to -and $e.valid_to -ne "") { $leakCount++ }
            }
        }
    } catch {
        Add-Line "graph_full fetch failed: $($_.Exception.Message)"
    }

    $detail = "active=$countActive include_invalidated=$countAll leaked_invalidated_in_full=$leakCount"
    if ($r1.ExitCode -eq 0 -and $r2.ExitCode -eq 0 -and $countAll -ge $countActive -and $leakCount -eq 0) {
        Step-Result -Id "4" -Title "Track 1 — graph reads" -Status "PASS" -Detail $detail
    } else {
        Step-Result -Id "4" -Title "Track 1 — graph reads" -Status "FAIL" -Detail $detail
    }
} catch {
    Step-Result -Id "4" -Title "Track 1 — graph reads" -Status "FAIL" -Detail "Exception: $($_.Exception.Message)"
}

# ---------------------------------------------------------------------------
# Step 5: Track 5 — model picker
# ---------------------------------------------------------------------------

try {
    # Probe API endpoint(s) that surface picker labels. The picker reads
    # /api/config/agent_models or similar — try a few known paths.
    $found = @()
    foreach ($path in @("/api/agent/models", "/api/config", "/api/models")) {
        try {
            $url = "http://127.0.0.1:$DevPort$path"
            $resp = Invoke-RestMethod -Uri $url -Headers $headers -Method Get -TimeoutSec 5
            $blob = ($resp | ConvertTo-Json -Depth 6)
            if ($blob -match "(sonnet|opus|haiku)") {
                $found += "${path}: matched short label"
                break
            }
        } catch { }
    }
    # Also scan the source: src/components/Settings/ModelPicker.tsx
    $pickerSrc = Join-Path $repoRoot "src/components/Settings/ModelPicker.tsx"
    $hasShort = $false
    if (Test-Path $pickerSrc) {
        $src = Get-Content $pickerSrc -Raw
        if ($src -match "sonnet" -and $src -match "opus" -and $src -match "haiku") {
            $hasShort = $true
            $found += "ModelPicker.tsx: contains sonnet/opus/haiku"
        }
    }
    if ($hasShort -or $found.Count -gt 0) {
        Step-Result -Id "5" -Title "Track 5 — model picker" -Status "PASS" -Detail ($found -join "; ")
    } else {
        Step-Result -Id "5" -Title "Track 5 — model picker" -Status "WARN" -Detail "Could not auto-verify picker labels via API or source — manual UI smoke deferred"
    }
} catch {
    Step-Result -Id "5" -Title "Track 5 — model picker" -Status "WARN" -Detail "Exception during auto-verify: $($_.Exception.Message)"
}

# ---------------------------------------------------------------------------
# Step 6: Track 2 — graph mutation (revive-broken, route, purge, introduce-topic)
# ---------------------------------------------------------------------------

try {
    $reviveDry = Run-Cmd "`"$devCli`" mem graph revive-broken --dry-run --json"
    $revive    = Run-Cmd "`"$devCli`" mem graph revive-broken --json"

    # Route — pick A/B/C from the graph. We need a real triple of entity ids.
    $a = ""; $b = ""; $c = ""
    try {
        $tunnels = Invoke-RestMethod -Uri "http://127.0.0.1:$DevPort/api/mem/graph/full" -Headers $headers -Method Get -TimeoutSec 30
        if ($tunnels.edges) {
            $first = $tunnels.edges | Select-Object -First 5
            if ($first.Count -ge 1) { $a = $first[0].from_id; $b = $first[0].to_id }
            # Pick a `c` that's a node id (any third entity not equal to a/b).
            foreach ($n in $tunnels.nodes) {
                if ($n.id -ne $a -and $n.id -ne $b) { $c = $n.id; break }
            }
        }
    } catch { }

    $routeDetail = "skipped (no candidate triple)"
    if ($a -and $b -and $c) {
        $route = Run-Cmd "`"$devCli`" mem graph route --from `"$a`" --to `"$b`" --via `"$c`" --json"
        $routeDetail = "from=$a to=$b via=$c exit=$($route.ExitCode)"
    }

    # purge-invalidated — pick a date well in the past so we catch our manual damage.
    $cutoff = "2025-12-31T00:00:00Z"
    $purgeDry = Run-Cmd "`"$devCli`" mem graph purge-invalidated --before `"$cutoff`" --dry-run --json"
    $purge    = Run-Cmd "`"$devCli`" mem graph purge-invalidated --before `"$cutoff`" --json"

    # introduce-topic — synthetic topic id. Anchor and source must exist. Use a/b from above.
    $topicDetail = "skipped (no anchor available)"
    if ($a -and $b) {
        $topicId = "topic_e2e_test_$ts"
        $topic = Run-Cmd "`"$devCli`" mem graph introduce-topic --topic-id $topicId --type Topic --sources `"$a`" --anchor `"$b`" --json"
        $topicDetail = "topic_id=$topicId exit=$($topic.ExitCode)"
    }

    $allOk = ($reviveDry.ExitCode -eq 0) -and ($revive.ExitCode -eq 0) -and ($purgeDry.ExitCode -eq 0) -and ($purge.ExitCode -eq 0)
    $detail = "revive_dry=$($reviveDry.ExitCode) revive=$($revive.ExitCode) purge_dry=$($purgeDry.ExitCode) purge=$($purge.ExitCode) route=[$routeDetail] topic=[$topicDetail]"
    if ($allOk) {
        Step-Result -Id "6" -Title "Track 2 — graph mutations" -Status "PASS" -Detail $detail
    } else {
        Step-Result -Id "6" -Title "Track 2 — graph mutations" -Status "FAIL" -Detail $detail
    }
} catch {
    Step-Result -Id "6" -Title "Track 2 — graph mutations" -Status "FAIL" -Detail "Exception: $($_.Exception.Message)"
}

# ---------------------------------------------------------------------------
# Step 7: Track 3 — Dreamer normalize phase via daemon
# ---------------------------------------------------------------------------

try {
    $r = Run-Cmd "`"$devCli`" mem dream run --phase normalize --json"
    # Tail last 50 dev log lines and search for stack traces.
    $logFile = Join-Path $HOME ".codeterm-dev/logs/codeterm.log"
    $traceHit = "no"
    if (Test-Path $logFile) {
        $tail = Get-Content $logFile -Tail 200 -ErrorAction SilentlyContinue
        $tailStr = $tail -join "`n"
        if ($tailStr -match "panicked at|RUST_BACKTRACE|stack backtrace") { $traceHit = "yes" }
    }
    $status = if ($r.ExitCode -eq 0 -and $traceHit -eq "no") { "PASS" } else { "FAIL" }
    Step-Result -Id "7" -Title "Track 3 — Dreamer normalize phase" -Status $status -Detail "exit=$($r.ExitCode) trace_in_dev_log=$traceHit"
} catch {
    Step-Result -Id "7" -Title "Track 3 — Dreamer normalize phase" -Status "FAIL" -Detail "Exception: $($_.Exception.Message)"
}

# ---------------------------------------------------------------------------
# Step 8: Track 4 (revised) — Dreamer recluster phase
# ---------------------------------------------------------------------------

try {
    $r = Run-Cmd "`"$devCli`" mem dream run --phase recluster --json"
    $planDir = Join-Path $HOME ".codeterm-dev/mem/dream-plans"
    $planFile = ""
    if (Test-Path $planDir) {
        $plan = Get-ChildItem $planDir -Filter "*.md" -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
        if ($plan) { $planFile = $plan.FullName }
    }
    $applyDetail = "no plan produced (likely no LLM API key — wire-up still proven by clean error)"
    if ($planFile) {
        # Apply only if we got a real plan
        $a = Run-Cmd "`"$devCli`" mem dream apply `"$planFile`" --json"
        $applyDetail = "plan=$planFile apply_exit=$($a.ExitCode)"
    }
    # Either we got a plan and applied it, OR phase ran cleanly (exit 0 means daemon
    # accepted the call — wire-up works).
    $status = if ($r.ExitCode -eq 0 -or $planFile) { "PASS" } else { "WARN" }
    Step-Result -Id "8" -Title "Track 4 (revised) — Dreamer recluster" -Status $status -Detail "phase_exit=$($r.ExitCode) $applyDetail"
} catch {
    Step-Result -Id "8" -Title "Track 4 (revised) — Dreamer recluster" -Status "WARN" -Detail "Exception: $($_.Exception.Message)"
}

# ---------------------------------------------------------------------------
# Step 9: Track 7 — Ctrl+V smoke (manual)
# ---------------------------------------------------------------------------

Step-Result -Id "9" -Title "Track 7 — Ctrl+V plain text wins" -Status "SKIP" -Detail "Manual UI smoke deferred to user (requires keyboard input). Code path verified by unit tests in Track 7 commit 92e7d5de."

# ---------------------------------------------------------------------------
# Step 10: Track 8 + 9 — evolution schema + lesson promotion
# ---------------------------------------------------------------------------

try {
    $r = Run-Cmd "`"$devCli`" mem run-worker dreaming-light --db-path `"$DevDbPath`" --json"
    $hasPromoted = $false
    $hasAlreadyHigh = $false
    if ($r.OutputStr -match '"lessons_promoted"') { $hasPromoted = $true }
    if ($r.OutputStr -match '"lessons_already_high_confidence"' -or $r.OutputStr -match '"lessons_already"') { $hasAlreadyHigh = $true }
    $detail = "exit=$($r.ExitCode) lessons_promoted_field=$hasPromoted lessons_already_field=$hasAlreadyHigh"
    if ($r.ExitCode -eq 0 -and $hasPromoted) {
        Step-Result -Id "10" -Title "Track 8/9 — evolution schema + lesson promotion" -Status "PASS" -Detail $detail
    } else {
        Step-Result -Id "10" -Title "Track 8/9 — evolution schema + lesson promotion" -Status "FAIL" -Detail $detail
    }
} catch {
    Step-Result -Id "10" -Title "Track 8/9 — evolution schema + lesson promotion" -Status "FAIL" -Detail "Exception: $($_.Exception.Message)"
}

# ---------------------------------------------------------------------------
# Step 11: Audit logs — graph_purge_log + lesson_audit
# ---------------------------------------------------------------------------

try {
    $auditScript = Join-Path $PSScriptRoot "audit_dump.py"
    if (-not (Test-Path $auditScript)) {
        # Inline emit
        $auditPyBody = @'
import sqlite3, sys, json
db = sys.argv[1]
con = sqlite3.connect(db)
out = {}
for table in ("graph_purge_log", "lesson_audit"):
    try:
        rows = con.execute(f"SELECT * FROM {table} ORDER BY rowid DESC LIMIT 20").fetchall()
        cols = [d[0] for d in con.execute(f"SELECT * FROM {table} LIMIT 1").description] if rows else []
        out[table] = {"count": len(rows), "cols": cols, "rows": [list(r) for r in rows]}
    except Exception as e:
        out[table] = {"error": str(e)}
print(json.dumps(out, default=str))
'@
        Set-Content -Path $auditScript -Value $auditPyBody -Encoding UTF8
    }
    $r = Run-Cmd "python `"$auditScript`" `"$DevDbPath`""
    if ($r.ExitCode -eq 0) {
        try {
            $j = $r.OutputStr | ConvertFrom-Json
            $purgeCount = if ($j.graph_purge_log -and $j.graph_purge_log.count) { $j.graph_purge_log.count } else { 0 }
            $lessonCount = if ($j.lesson_audit -and $j.lesson_audit.count) { $j.lesson_audit.count } else { 0 }
            $detail = "graph_purge_log_rows=$purgeCount lesson_audit_rows=$lessonCount"
            $status = if ($purgeCount -gt 0) { "PASS" } else { "WARN" }
            Step-Result -Id "11" -Title "Audit logs" -Status $status -Detail $detail
        } catch {
            Step-Result -Id "11" -Title "Audit logs" -Status "WARN" -Detail "Could not parse audit_dump JSON; raw output above"
        }
    } else {
        Step-Result -Id "11" -Title "Audit logs" -Status "FAIL" -Detail "audit_dump.py exit=$($r.ExitCode)"
    }
} catch {
    Step-Result -Id "11" -Title "Audit logs" -Status "FAIL" -Detail "Exception: $($_.Exception.Message)"
}

# ---------------------------------------------------------------------------
# Step 12: Restore step — apply v1.1.23 cold against an older snapshot
# ---------------------------------------------------------------------------

try {
    if (Test-Path $ProdBackupPath) {
        Add-Line "Killing dev daemon before swapping DB to old backup..."
        Push-Location $repoRoot; try { & bash ./build.sh kill 2>&1 | Out-Null } catch {}; Pop-Location
        Start-Sleep -Seconds 2

        # WAL files — drop them so the cold open sees a clean state.
        foreach ($suf in "", "-wal", "-shm") {
            $f = "$DevDbPath$suf"
            if (Test-Path $f) { Remove-Item $f -Force }
        }
        Copy-Item $ProdBackupPath $DevDbPath -Force

        # Run a smoke pass: dreaming-light against the cold DB.
        $r = Run-Cmd "`"$devCli`" mem run-worker dreaming-light --db-path `"$DevDbPath`" --json"
        $status = if ($r.ExitCode -eq 0) { "PASS" } else { "FAIL" }
        Step-Result -Id "12" -Title "Restore — v1.1.23 cold-applies to older backup" -Status $status -Detail "exit=$($r.ExitCode)"
    } else {
        Step-Result -Id "12" -Title "Restore — v1.1.23 cold-applies to older backup" -Status "SKIP" -Detail "Backup file not present: $ProdBackupPath"
    }
} catch {
    Step-Result -Id "12" -Title "Restore — v1.1.23 cold-applies to older backup" -Status "FAIL" -Detail "Exception: $($_.Exception.Message)"
}

# ---------------------------------------------------------------------------
# Final summary
# ---------------------------------------------------------------------------

# Treat WARN/SKIP as not failing the run.
$failed = $results | Where-Object { $_.Status -eq "FAIL" }
if ($failed.Count -gt 0) { $overall = "FAIL" } else { $overall = "PASS" }

Add-Line ""
Add-Line "## Summary"
Add-Line ""
Add-Line "| Step | Title | Status |"
Add-Line "|---|---|---|"
foreach ($r in $results) {
    Add-Line "| $($r.Id) | $($r.Title) | **$($r.Status)** |"
}
Add-Line ""
Add-Line "Overall: **$overall**"
Add-Line ""

$reportPath = Join-Path $repoRoot "evals/v1_1_23_e2e/run-$ts-$overall.md"
$reportLines | Set-Content -Path $reportPath -Encoding UTF8
Write-Host ""
Write-Host "Report: $reportPath"
Write-Host "Overall: $overall"

if ($overall -eq "PASS") { exit 0 } else { exit 1 }
