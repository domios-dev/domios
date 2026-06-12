# ISSUE-131 — keepawake/caffeinate orphan leaks when daemon exits abruptly

**Executor:** Gpt5.5 docsync (04a7680e) · **Branch:** stabilization · **Effort:** M
**Source:** `gh issue view 131` (macOS forensics: caffeinate reparented to launchd after CodeTerm exits)

## Goal
Keepawake child processes (macOS `caffeinate`) never outlive the daemon: a clean exit kills them, and an abrupt exit/crash leaves nothing orphaned that a normal restart can't reclaim.

## Step 0 — confirm receipt
```
codeterm send-text "TASK ISSUE-131 ACCEPTED, starting" --pane 5c64a651
```

## Evidence (run FIRST; report BLOCKED if stale)
```
gh issue view 131
git grep -ni "caffeinate\|keepawake\|SetThreadExecutionState\|ExecutionState" -- src-tauri/src | head -20
```

## Scope
- Find the keepawake implementation. macOS spawns `caffeinate` as a CHILD process; Windows uses `SetThreadExecutionState` (no child, in-process — so the leak is macOS-specific, but the fix lives in cross-platform shutdown handling).
- The leak: when the daemon exits abruptly, the `caffeinate` child is reparented to launchd (ppid=1) and keeps running. Fix options (pick the robust one, justify): (a) on macOS, spawn caffeinate in the daemon's process group and ensure the group is signalled on exit; (b) track the child PID persistently (e.g. in the dev/prod data dir) and on STARTUP reap any stale keepawake child recorded from a previous run that is no longer wanted; (c) a Drop guard that kills the child on graceful shutdown (covers clean exit but NOT SIGKILL/crash — so combine with (b) for the abrupt case). The issue is specifically about ABRUPT exit, so a startup-reclaim (b) is likely required, not just Drop.
- Keep it cross-platform: Windows in-process execution state needs no child reaping; guard the macOS-specific code per the cross-platform rules in CLAUDE.md.
- Unit-test the reclaim logic (e.g. a recorded-stale-pid reaper given a fake pid file). Full macOS runtime verification is deferred to the Air/Mac-Air lane — note that in the report.

## Verify (paste real output)
```
cargo test --manifest-path src-tauri/Cargo.toml <keepawake filters>   # must: pass
& "C:\Program Files\Git\bin\bash.exe" build.sh check                  # exit 0
```

## Forbidden
- NO git mutations. NO dev:7686. No killing processes by name globally (`pkill caffeinate` would kill the user's own caffeinate) — only the daemon's OWN tracked child.

## Report
```
codeterm send-text "TASK ISSUE-131 STATUS ... APPROACH: ... CHANGED: ... VERIFIED: ... MACOS-VERIFY: deferred-to-Air ..." --pane 5c64a651
```
