# SPLIT-CLI-MEM-MOD — shrink cli/src/cmd/mem/mod.rs below 800 lines

**Executor:** Bardeen (a696bebf, Spark) · **Branch:** stabilization · **Effort:** S
**Backlog ref:** SUITE-SPLIT (file-size drains) in ../stabilization-backlog.md

## Goal
`cli/src/cmd/mem/mod.rs` (currently 1111 lines) becomes a facade under 800 lines; the moved code lives in new concrete files inside `cli/src/cmd/mem/`. ZERO behavior change — this is a pure mechanical move.

## Step 0 — confirm receipt (ALWAYS, before anything)
```
codeterm send "TASK SPLIT-CLI-MEM-MOD ACCEPTED, starting" --pane 5c64a651
```

## Evidence (run FIRST; if output differs, report BLOCKED "evidence stale")
```
git log --oneline -1        # expected: 2c077721 or a descendant
git grep -c "" -- cli/src/cmd/mem/mod.rs   # if this fails in PowerShell use: (Get-Content cli/src/cmd/mem/mod.rs | Measure-Object -Line).Lines  -> expected ~1111
```

## Rules (read twice)
- MOVE code, never rewrite it. Cut a coherent block (a group of handler functions for one subcommand family, e.g. graph/secret/skill/worker), paste it into a new file `cli/src/cmd/mem/<family>.rs`, add `mod <family>;` + the minimal `use` / `pub(crate)` adjustments in mod.rs. Repeat until mod.rs < 800.
- Do NOT change any function body, string, or logic. Do NOT rename anything. Do NOT reorder match arms.
- If a `#[cfg(test)] mod tests` block is inline, keep `mod tests;` in place — tests already live in their own file here; do not touch them beyond imports if needed.
- NO formatter sweeps: run `cargo fmt` only on the files you created/edited is impossible per-file — instead just match existing style by hand; the pre-push hook runs fmt check on my side.
- NO git commands of any kind. NO other files.

## Verify (run all; paste real output in the report)
```
cargo test --manifest-path cli/Cargo.toml                      # must: 133+ passed, 0 failed
& "C:\Program Files\Git\bin\bash.exe" -c "cd /c/Developer/codeterm && bash build.sh check"   # must: exit 0
& "C:\Program Files\Git\bin\bash.exe" -c "cd /c/Developer/codeterm && bash scripts/check-file-sizes.sh --fail"  # must: exit 0 AND cli/src/cmd/mem/mod.rs no longer listed in the advisory mod.rs section
(Get-Content cli/src/cmd/mem/mod.rs | Measure-Object -Line).Lines   # must: < 800
```

## Report
```
codeterm send "TASK SPLIT-CLI-MEM-MOD STATUS ... CHANGED: <every file incl. new ones> VERIFIED: <paste outputs> NOTES: ..." --pane 5c64a651
```
