# ISSUE-132 — dispatch::tests::pane_list_focused_field_tracks_mux_focused_pane_id fails

**Executor:** Gpt5.5 modelvalidate (352d7ecd) · **Branch:** stabilization · **Effort:** M
**Source:** `gh issue view 132`

## Goal
The test passes honestly — either the `focused` field in `build_pane_list_json` is fixed to track `mux.focused_pane_id`, or the test is corrected to match the real (correct) focus semantics. No green-by-deleting-the-assert.

## Step 0 — confirm receipt
```
codeterm send-text "TASK ISSUE-132 ACCEPTED, starting" --pane 5c64a651
```

## Evidence (run FIRST; report BLOCKED if stale)
```
gh issue view 132
cargo test --manifest-path src-tauri/Cargo.toml pane_list_focused_field 2>&1 | Select-String "FAILED|panicked|test result"   # expected: currently FAILS at "pane_a must be unfocused after switch"
```

## Scope
- Failure: after `state.set_focused_pane(&pane_b)`, `pane_a` still reports `focused=true` (test at src-tauri/src/dispatch/mod.rs:770-775).
- Determine the TRUTH first: is focus a single global `mux.focused_pane_id` (only one pane focused anywhere), or per-workspace (each workspace has its own focused pane)? Read `set_focused_pane`, `focused_pane_id`, and how `build_pane_list_json` derives the `focused` bool. The test creates pane_a and pane_b — confirm whether they land in the SAME workspace (if `create_pane` makes separate workspaces, per-workspace focus would legitimately keep pane_a focused in its own ws, making the TEST wrong, not the code).
- Fix whichever is actually wrong:
  - If code: `build_pane_list_json` must set `focused=true` only for the single mux-focused pane; `set_focused_pane` must clear prior focus. Fix it; the test stands.
  - If test: the panes must be in one workspace (or assert per-workspace focus semantics correctly). Fix the test to assert the real contract, and document the contract in a one-line comment.
- State your determination explicitly in the report with the code evidence.

## Verify (paste real output)
```
cargo test --manifest-path src-tauri/Cargo.toml pane_list_focused_field   # must: pass
& "C:\Program Files\Git\bin\bash.exe" build.sh check                       # exit 0
```

## Forbidden
- NO git mutations. NO dev:7686. Do not weaken the assertion to force green — fix the real mismatch.

## Report
```
codeterm send-text "TASK ISSUE-132 STATUS ... DETERMINATION: code-bug|test-bug + evidence ... CHANGED: ... VERIFIED: ..." --pane 5c64a651
```
