# DETECT-MODEL-FIX — pane model detection must attribute the pane's OWN session

**Executor:** Gpt5.5 modelprobe (77e5f8be) · **Branch:** stabilization · **Effort:** M
**Backlog ref:** ../model-fidelity-report.md (your own probe, arbiter-verified)

## Goal
`pane.agent_session_model` never shows another pane's model: codex gpt-5.4-mini panes stop reporting GPT-5.5, opencode non-default-model panes stop reporting big-pickle/whatever-was-latest-globally, and detection failures stay null instead of guessing.

## Step 0 — confirm receipt (ALWAYS, before anything)
```
codeterm send "TASK DETECT-MODEL-FIX ACCEPTED, starting" --pane 5c64a651
```

## Evidence (run these FIRST; if output differs, report BLOCKED "evidence stale")
```
git log --oneline -1 -- docs/review-2026-06/model-fidelity-report.md   # expected: report committed at HEAD
git grep -n "detect_session_model" -- src-tauri/src | head -8          # expected: provider trait + per-provider impls
```

## Known facts (from your probe — do not re-derive, do verify)
- opencode `detect_session_model` reads the DB's LATEST assistant model globally (provider.rs:485-488) — not the pane's own session. Cross-pane contamination is the suspected mechanism for both observed mismatches.
- codex pane detected `GPT-5.5` while running gpt-5.4-mini — find whether it claimed a foreign session file or normalizes wrongly.
- claude/codex sometimes leave model null even when the session is claimed — decide whether that's acceptable (null = honest unknown) and document; null is BETTER than wrong.

## Steps (in order, no skipping, no additions)
1. Root-cause each of the two MISMATCH rows from the report table: trace which session/DB row the detection actually read for panes 887d7621-style (codex) and 7ea85f4d-style (opencode). Name the exact wrong join/lookup.
2. Fix: detection must key off the pane's OWN claimed session id (codex: the session file belonging to the pane's sid; opencode: the session/messages scoped to the pane's session, not global latest). If a pane has no claimed session yet → model stays null. NO fallback to "latest anything".
3. Unit tests per provider impl: own-session attribution (two concurrent sessions with different models → each pane reports its own), no-session → null, and the regression shape from the probe (newest global record belongs to someone else → must NOT win).
4. Structural-fingerprint rule applies (CLAUDE.md): no literal display-string parsing beyond what already exists; key on session ids/records.

## Verify (run all; paste real output in the report)
```
cargo test --manifest-path src-tauri/Cargo.toml <narrow filters for the new tests>   # must: pass (PowerShell foreground, 90s cap)
& "C:\Program Files\Git\bin\bash.exe" build.sh check                                  # must: exit 0
& "C:\Program Files\Git\bin\bash.exe" -c 'bash scripts/check-file-sizes.sh --fail'    # must: exit 0
```
Live re-probe (PROD, one disposable workspace under C:/Temp, same method as before, claude excluded this time): codex gpt-5.4-mini + opencode deepseek-v4-flash must both report their OWN model or null — never a foreign one. NOTE: prod runs the OLD daemon — your fix won't be live there, so the live re-probe is DEFERRED to the dev window; say so in the report instead of faking it.

## Forbidden (beyond the global rules in README.md)
- NO git mutations. NO touching evals/, tests/e2e (the e2e for this lands with CRON-DREAM-COV-style dev window later).
- Don't widen scope into spawn validation — that's SPAWN-MODEL-VALIDATE, another lane.

## Report
```
codeterm send "TASK DETECT-MODEL-FIX STATUS ... ROOT CAUSE: ... CHANGED: ... VERIFIED: ... NOTES: ..." --pane 5c64a651
```
