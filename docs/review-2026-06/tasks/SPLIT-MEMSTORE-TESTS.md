# SPLIT-MEMSTORE-TESTS — shrink crates/mem-store/src/tests/mod.rs below 800 lines

**Executor:** Hopper (c0bf209e, Spark) · **Branch:** stabilization · **Effort:** S
**Backlog ref:** SUITE-SPLIT (file-size drains) in ../stabilization-backlog.md

## Goal
`crates/mem-store/src/tests/mod.rs` (currently ~1072 lines) becomes a thin module index under 800 lines; test bodies move into themed sibling files in `crates/mem-store/src/tests/`. ZERO test change — pure mechanical move, every test keeps its name and body.

## Step 0 — confirm receipt (ALWAYS, before anything)
```
codeterm send-text "TASK SPLIT-MEMSTORE-TESTS ACCEPTED, starting" --pane 5c64a651
```

## Evidence (run FIRST; if output differs, report BLOCKED "evidence stale")
```
git log --oneline -1        # expected: 2c077721 or a descendant
(Get-Content crates/mem-store/src/tests/mod.rs | Measure-Object -Line).Lines   # expected: ~1072
cargo test -p mem-store 2>&1 | Select-String "test result"                      # expected: all pass - note the exact count, it must be IDENTICAL after the split
```

## Rules (read twice)
- Group tests by theme (look at function-name prefixes and what they exercise: graph / revive / search / store-crud / etc). Cut each group into `crates/mem-store/src/tests/<theme>.rs`, declare `mod <theme>;` in tests/mod.rs, move the needed `use` lines with them (helpers shared by several groups stay in mod.rs, made `pub(crate)` or `super::`-visible).
- Do NOT modify any test body, assertion, or name. Do NOT merge, delete, or "improve" tests.
- The test COUNT from `cargo test -p mem-store` must be exactly the same before and after — paste both.
- NO git commands. NO files outside crates/mem-store/src/tests/.

## Verify (run all; paste real output in the report)
```
cargo test -p mem-store     # must: same pass count as your Evidence run, 0 failed
& "C:\Program Files\Git\bin\bash.exe" -c "cd /c/Developer/codeterm && bash scripts/check-file-sizes.sh --fail"  # must: exit 0 AND tests/mod.rs gone from the advisory section
(Get-Content crates/mem-store/src/tests/mod.rs | Measure-Object -Line).Lines   # must: < 800
```

## Report
```
codeterm send-text "TASK SPLIT-MEMSTORE-TESTS STATUS ... CHANGED: <every file incl. new ones> VERIFIED: <paste outputs incl. before/after test counts> NOTES: ..." --pane 5c64a651
```
