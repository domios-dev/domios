# WEB-COV-FAST — web gate auth-boundary e2e (fast tier)

**Executor:** Gpt5.5 chatui (54499ecb) · **Branch:** stabilization · **Effort:** S/M
**Backlog ref:** WEB-COV in ../stabilization-backlog.md (fast tier only; the Playwright tier waits for the restore-matrix lane to land its harness changes)

## Goal
The token-link web gate has honest non-llm e2e coverage: unauthenticated and bad-token requests are rejected, a valid token serves the SPA and grants API access, and the token actually gates every web entry surface (not just one route).

## Step 0 — confirm receipt (ALWAYS, before anything)
```
codeterm send-text "TASK WEB-COV-FAST ACCEPTED, starting" --pane 5c64a651
```

## Evidence (run these FIRST; if output differs, report BLOCKED "evidence stale")
```
git log --oneline -1                                  # expected: a4fdeb96 or a descendant
git grep -c "fn" -- src-tauri/src/api/web_routes.rs   # expected: nonzero (web routes exist)
```

## Scope (fast tier ONLY)
- New `tests/e2e/test_web_gate.py` against fresh dev (7686), dev-locked conftest.
- Read `src-tauri/src/api/web_routes.rs` first and enumerate every route the web gate serves (SPA entry, assets, any web-scoped API). The test must cover the full enumerated set, not a sample — list them in the report.
- Cases per the honesty rules (docs/contributing/writing-tests.md):
  1. No token → must NOT serve the SPA (assert status code exactly; assert response body does NOT contain the SPA bundle marker).
  2. Garbage/expired token → same rejection, exact status.
  3. Valid token (obtain the same way `codeterm web link` does — read the code, don't hardcode) → SPA served (assert a real bundle marker present, payload length sane) AND an authenticated API call through the web path succeeds with exact-shape response.
  4. Token must not leak: the rejection response must not echo the expected token.
- NO Playwright in this task. NO touching tests/playwright/, evals/, or web frontend code.
- If the gate has a deliberate open surface (e.g. PWA manifest), assert that explicitly as an allowed-anonymous list, not a silent pass.

## Verify (run all; paste real output in the report)
```
python -m pytest tests/e2e/test_web_gate.py -q        # must: all pass against fresh dev
& "C:\Program Files\Git\bin\bash.exe" build.sh kill   # leave no dev running
```

## Forbidden (beyond the global rules in README.md)
- NO git mutations — report changed paths + proposed commit message.
- No daemon code changes; if you find a real auth hole, STOP and report it as a finding instead of fixing.

## Report
```
codeterm send-text "TASK WEB-COV-FAST STATUS ... CHANGED: ... VERIFIED: ... ROUTES COVERED: ... NOTES: ..." --pane 5c64a651
```
