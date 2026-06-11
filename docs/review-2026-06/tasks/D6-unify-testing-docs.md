# D6 — Single source of truth for testing docs

**Executor:** Gpt5.5 pane · **Branch:** stabilization · **Effort:** S/M
**Backlog ref:** D6 in ../stabilization-backlog.md

## Step 0 — confirm receipt (ALWAYS, before anything)
```
codeterm send-text "TASK D6 ACCEPTED, starting" --pane 5c64a651
```

## Goal
Exactly ONE document defines the test-layer taxonomy: `docs/TESTING.md`. Other test
docs keep operational detail but link up instead of redefining layers. A "which
layer do I write?" decision tree exists in TESTING.md.

## Evidence (run FIRST)
```bash
head -15 docs/TESTING.md                      # currently defines 3 layers (Unit/E2E/UI)
head -5 tests/playwright/README.md            # calls Playwright "e2e" — contradiction
ls tests/TESTPLAN.md 2>&1                     # must: No such file (archived by D1; if it still exists, report BLOCKED)
sed -n '1,80p' docs/review-2026-06/testing-process.md   # §1 = the approved taxonomy to import
```

## Steps
1. Rewrite the layer section of `docs/TESTING.md` using the taxonomy TABLE and the
   DECISION TREE from `docs/review-2026-06/testing-process.md` §1 (copy the content,
   adapt wording freely, do not invent new layers). Keep the rest of TESTING.md's
   operational content (commands, fixtures, prompt-detection guide) — reconcile, not
   rewrite from scratch.
2. `tests/playwright/README.md`: replace its own taxonomy/layer claims with one
   line linking to docs/TESTING.md; KEEP all operational content (how to run, CDP
   notes, tag definitions, the no-CI decision rationale).
3. Check `docs/contributing/agent-restart-testing.md` and
   `docs/contributing/voice-transcription-testing.md` headers — if they define
   layers, add the same link-up line; otherwise leave untouched.
4. Add to docs/TESTING.md a short "Runbook" table: layer → exact command → when
   required (source the commands from testing-process.md §5; mark `test:all` as
   "coming — task D2" since it does not exist yet — do NOT document unbuilt
   commands as real).
5. Bump revision tags on every file you touched.
6. Commit: `docs: unify test-layer taxonomy in TESTING.md (single source of truth)`

## Verify
```bash
grep -c "Unit\|unit" docs/TESTING.md >/dev/null && echo ok
grep -n "TESTING.md" tests/playwright/README.md     # must: >=1 hit (the link-up)
# No second taxonomy: grep -in "three layers\|3 layers\|test layers" tests/ docs/contributing/ --include="*.md" → only links, no definitions (paste output)
```

## Forbidden
- Do not change playwright config, specs, or any code — docs only.
- Do not document commands that don't exist yet as if they do.
- Do not edit docs/review-2026-06/ (arbiter's ledger; you READ from it only).

## Report
```
codeterm send-text "TASK D6 STATUS ... CHANGED: ... VERIFIED: ... NOTES: ..." --pane 5c64a651
```
