# SHRINK-CLI-MAIN — cli/src/main.rs grew past its allowlist baseline

**Executor:** Bardeen (a696bebf, Spark) · **Branch:** stabilization · **Effort:** S · **Release-blocking** (CI `check` job fails)
**Backlog ref:** file-size discipline

## Goal
`cli/src/main.rs` drops back to or below its allowlist baseline of **1041** lines (currently ~1076). Pure mechanical move — zero behavior change. The CI `check` job (`scripts/check-file-sizes.sh --fail`) rejects an allowlisted file that GREW; it must shrink.

## Step 0 — confirm receipt
```
codeterm send-text "TASK SHRINK-CLI-MAIN ACCEPTED, starting" --pane 5c64a651
```

## Evidence (run FIRST; report BLOCKED if stale)
```
(Get-Content cli/src/main.rs | Measure-Object -Line).Lines      # expected: ~1076 (over the 1041 baseline)
Select-String -Path scripts/file-size-allowlist.txt -Pattern "cli/src/main.rs"   # expected: cli/src/main.rs  1041
```

## Rules (read twice — same discipline as your cli/mem split)
- MOVE code, never rewrite. Identify a coherent block in main.rs — most likely a group of inline command-dispatch arms or a helper cluster (e.g. the workspace/send/config local handlers) — and move it into a new or existing file under `cli/src/cmd/` or `cli/src/`, leaving a thin call from main.rs. The recent growth came from the workspace-create reuse-notice and send/stdin handling; those local handlers are good extraction candidates.
- Do NOT change any logic, string, flag, or arm order. Do NOT touch scripts/file-size-allowlist.txt.
- Keep it compiling green at every step. NO git commands.

## Verify (paste real output)
```
cargo test --manifest-path cli/Cargo.toml                       # must: 141 passed, 0 failed
& "C:\Program Files\Git\bin\bash.exe" -c "cd /c/Developer/codeterm && bash build.sh check"   # exit 0
& "C:\Program Files\Git\bin\bash.exe" -c "cd /c/Developer/codeterm && bash scripts/check-file-sizes.sh --fail"   # exit 0, cli/src/main.rs no longer flagged
(Get-Content cli/src/main.rs | Measure-Object -Line).Lines      # must: <= 1041
```

## Report
```
codeterm send-text "TASK SHRINK-CLI-MAIN STATUS ... CHANGED: <files incl new> main.rs <N> lines VERIFIED: ... " --pane 5c64a651
```
