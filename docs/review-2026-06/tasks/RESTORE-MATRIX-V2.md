# RESTORE-MATRIX-V2 — one-workspace provider restore matrix with UI-visible oracle

**Executor:** Gpt5.5 (a10ccdbd) · **Branch:** stabilization · **Effort:** L
**Backlog ref:** RESTORE-MATRIX in ../stabilization-backlog.md (supersedes the broad session-restore flow as acceptance)

## Goal
Restore correctness is proven by a focused eval: ONE workspace, 5 panes per system provider (claude/codex/opencode), unique UUID marker in each, hard dev restart, and a Playwright assertion that each marker is VISIBLE in its own pane's terminal after restore — plus sid identity and cross-contamination negatives. The old broad eval is demoted to a fail-closed smoke.

## Step 0 — confirm receipt (ALWAYS, before anything)
```
codeterm send "TASK RESTORE-MATRIX-V2 ACCEPTED, starting" --pane 5c64a651
```

## Evidence (run these FIRST; if output differs, report BLOCKED "evidence stale")
```bash
git log --oneline -1                          # expected: 3478012d security: refuse verified text injection...
git status --short                            # expected: only M evals/session-restore/run.sh + baseline.json (your held churn)
curl -s http://127.0.0.1:7686/api/health      # dev may be down; that's fine, run.sh starts it
```

## Design (decided, do not re-litigate)
- **New eval** `evals/restore-matrix/` (run.sh + README.md). Do NOT extend the old broad flow.
- **One workspace** `RESTORE-MATRIX-<runid>` created via `dc` (source evals/_lib/lib.sh; dev 7686 only).
- **15 panes**: 5 × claude (haiku — explicitly authorized, one cheap prompt each), 5 × codex (gpt-5.4-mini), 5 × opencode (big-pickle).
- **Marker**: per-pane UUID generated at runtime. Send via verified send: `Reply with exactly RESTORE-MARKER-<uuid> and nothing else.` The user message itself carries the UUID, so the restored transcript shows it even if the model's reply failed.
- **Availability probe + skip policy**: before fanning out 5 panes for a provider, spawn ONE probe pane and send the marker. Provider is SKIPPED (entire provider, explicit line `SKIP provider=<p> reason=<...>`) when: marker does not appear in get-output within timeout, OR send fails with the new `UnsafePromptVisible` inject-safety error (rate-limit/account menu — that guard is your detector, never work around it), OR an auth/login screen is on screen. Skips never count as pass. If ALL three providers skip → exit 1 (nothing was tested).
- **Snapshot before kill**: pane→{provider, agent_session_id, marker} map from `/api/panes`, written to a run manifest JSON.
- **Hard restart**: `bash build.sh kill` then `bash build.sh dev --wait`. Never touch 7685.
- **Post-restore asserts, PER PANE (no aggregate booleans)**:
  - pane id exists, provider unchanged;
  - sid_after == sid_before for providers that persist sids; if a provider genuinely doesn't support sid resume, print `SKIP sid-check provider=<p> reason=unsupported` — verify what opencode actually supports, don't assume;
  - cross-contamination negatives: no pane carries another pane's sid; each pane's post-restart output contains ITS marker and NOT any other pane's marker.
- **UI-visible oracle (the user's core demand)**: Playwright spec `tests/playwright/suites/<pick per D3 tag contract>/restore-matrix.spec.ts`, tagged so it is NOT in the default fast set. It reads the manifest path from an env var, opens the matrix workspace in the dev UI, iterates its tabs, and asserts each pane's terminal renders that pane's exact UUID. run.sh invokes it after the API-level asserts. Metadata matching alone is NOT acceptance.
- **Honesty rules apply** (docs/contributing/writing-tests.md): missing precondition (playwright deps, dev won't start) = FAIL, not skip; all asserts against POST-restart state; cleanup closes the workspace on success, keeps it on failure and prints pane ids for forensics.

## Steps (in order, no skipping, no additions)
1. Decide salvage: anything in your held uncommitted `evals/session-restore/{run.sh,baseline.json}` worth folding into the new eval? Fold it there. Then make the OLD `evals/session-restore/run.sh` fail-closed: any printed `FAIL` line must produce nonzero exit (it currently prints FAILs and still passes — that's the false positive the user caught). Report whether baseline.json churn should be kept or reset.
2. Build `evals/restore-matrix/run.sh` + README.md per the design above.
3. Build the Playwright spec + manifest plumbing.
4. Run the full eval twice (idempotency): both runs must exit 0 with at least one provider not skipped.

## Verify (run all; paste real output in the report)
```bash
bash evals/restore-matrix/run.sh              # must: exit 0, per-pane PASS lines, explicit SKIP lines with reasons
bash evals/restore-matrix/run.sh              # must: exit 0 again (own workspace, own markers)
bash evals/session-restore/run.sh --check; echo $?   # old eval: must now exit nonzero if any FAIL printed
bash build.sh kill                            # leave no dev running; prod 7685 untouched
```

## Forbidden (beyond the global rules in README.md)
- NO git mutations of any kind — the SOCKET-HANG lane self-committed yesterday; that was a protocol violation. You report paths + proposed commit message, I commit.
- No raw `codeterm` in eval scripts — `dc` only.
- No weakening/bypassing the inject-safety guard; it is the skip detector.
- No baseline-tolerance pass logic in the new eval — exact per-pane asserts only.

## Report
```
codeterm send "TASK RESTORE-MATRIX-V2 STATUS ... CHANGED: ... VERIFIED: <per-pane table: pane/provider/marker/sid before-after/PASS-FAIL-SKIP+reason> NOTES: ..." --pane 5c64a651
```
