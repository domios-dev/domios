# D1 — Remove orphaned shell test scripts (incl. prod-port 7685 hazard)

**Executor:** Haiku or Sonnet pane · **Branch:** stabilization · **Effort:** S
**Backlog ref:** D1 in ../stabilization-backlog.md

## Goal
`tests/` root contains only directories and configs; no script anywhere under
`tests/` references prod port 7685.

## Evidence (run FIRST)
```bash
grep -n "7685" tests/e2e.sh                            # expected: hit at ~line 7
ls tests/*.sh tests/*.ps1 tests/*.ts tests/TESTPLAN.md # expected: ~12 loose files
```

## Steps
1. For EACH loose file in `tests/` root (`e2e.sh, final_e2e.sh, full-eval.sh,
   llm-eval.sh, llm-natural-eval.sh, orchestrator-eval.sh, agent_scenario.sh,
   test_app.sh, restart-clean.sh, shot.ps1, parser-test.ts, TESTPLAN.md`), run:
   `grep -rn "<filename>" docs/ scripts/ build.sh build.ps1 .github/ evals/ tests/ --include="*" | grep -v "tests/<filename>"`
   Record: referenced anywhere? (build a 12-row table for the report).
2. Unreferenced files → `git rm` them. Referenced files → list in the report,
   do NOT delete, ask the arbiter.
3. `TESTPLAN.md`: prepend the header
   `> Historical snapshot (2026-03-18). Findings may since be fixed; do not action without re-verifying.`
   and `git mv` it to `docs/review-archive/TESTPLAN-2026-03.md` (create the dir).
4. Commit: `test: remove orphaned shell test scripts (prod-port hazard) + archive TESTPLAN`

## Verify
```bash
grep -rn "7685" tests/                                 # must: zero executable hits
ls tests/                                              # must: only dirs + playwright config files
./build.sh check                                       # must: exit 0 (nothing referenced them)
```

## Forbidden
- Do not touch `tests/playwright/`, `tests/e2e/`, `tests/manual/`,
  `tests/fixtures/` contents — directories stay as-is. `tests/webdriver/` was
  removed after this task record was written.
- Do not delete a file that step 1 found referenced — report it instead.

## Report
```
codeterm send "TASK D1 STATUS ... CHANGED: ... VERIFIED: ... NOTES: <the 12-row reference table>" --pane 5c64a651
```
