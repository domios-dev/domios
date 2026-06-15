# T-RAT — e2e suite rationalization (no empty terminals, no vacuous asserts)

**Executor:** Gpt5.5 chatui (54499ecb) · **Branch:** stabilization · **Effort:** L
**Backlog ref:** T-RAT in ../stabilization-backlog.md (user-reported: "куча пустых терминалов", 40-min runs)

## Goal
The pytest e2e suite opens a terminal ONLY for tests that type into one, API-shape tests run against a pure REST client with no pane, every output assertion is echo-proof, and suite wall-time drops measurably.

## Step 0 — confirm receipt (ALWAYS, before anything)
```
codeterm send-text "TASK T-RAT ACCEPTED, starting" --pane 5c64a651
```

## CRITICAL COORDINATION RULE
The restore-matrix lane owns dev:7686 right now (constant kill/restart cycles). You may NOT start a dev daemon or run the e2e suite until I give you a dev window. Steps 1–4 are dev-free (reading + editing). When ready to measure/verify, send: `codeterm send-text "T-RAT requests dev window" --pane 5c64a651` and WAIT for my go.

## Evidence (run these FIRST; if output differs, report BLOCKED "evidence stale")
```
git log --oneline -1                          # expected: 57983758 or a descendant
python -m pytest tests/e2e --collect-only -q 2>$null | Select-Object -Last 2   # expected: a test count, no collection errors
```

## Steps (in order; 1–4 are dev-free)
1. AUDIT: classify every test in tests/e2e/: needs-PTY (types into a terminal / asserts screen) vs API-only (REST/socket shapes, config, metadata). Produce the table in your report.
2. API-ONLY FIXTURE: add a no-pane fixture (pure REST client, no workspace/terminal creation). Migrate every API-only test to it. After this, no terminal exists that no test types into.
3. PTY SHARING: needs-PTY tests share a module-scoped eager pane where tests don't mutate shell state; fresh eager pane only where state is mutated (cwd changes, env, running process). Keep isolation honest — when in doubt, fresh.
4. VACUITY SWEEP: every wait_for_output/contains assertion that could match the typed command's terminal echo is vacuous. Convention per docs/contributing/writing-tests.md: split markers so the asserted string exists only in real output (PowerShell: `Write-Output ('LEFT'+'RIGHT')`). Sweep ALL of tests/e2e and fix each instance; list them in the report.
5. MEASURE (requires dev window — ask first): full suite wall-time before (HEAD~ via stash or a clean checkout NOT in this tree — if that's not possible cleanly, take the before-number from the last known full run and say so explicitly) and after. Paste both numbers.
6. VERIFY (same dev window): full `python -m pytest tests/e2e -q -m "not llm"` green; second consecutive run also green (idempotency).

## Forbidden (beyond the global rules in README.md)
- NO git mutations. NO touching evals/, tests/playwright/, conftest auth/dev-lock logic (keep the poisoned-env protection intact — you may extend fixtures, not weaken the lock).
- No deleting tests to make the suite faster — classification, sharing, and fixture economy only. If a test is genuinely worthless, list it under "proposed deletions" in the report instead.
- No dev daemon outside the granted window.

## Report
```
codeterm send-text "TASK T-RAT STATUS ... CHANGED: ... CLASSIFICATION: <N api-only, M needs-pty, K shared-pane> VACUITY: <list> TIMING: before Xs / after Ys VERIFIED: ... NOTES: ..." --pane 5c64a651
```
