# CRON-DREAM-COV — cron firing + dreamer pipeline get real coverage

**Executor:** TBD (queued — needs the dev:7686 window after restore-matrix) · **Branch:** stabilization · **Effort:** M/L
**Backlog ref:** new (user-requested 2026-06-12); motivated by the June-8 prod incident: dreamers were dead for TWO WEEKS and nothing noticed.

## Current coverage facts (verified)
- tests/e2e/test_cron.py: CRUD + trigger, but trigger only asserts a history row exists (shape-only, `"id" in result or "status" in result`). No assert that an agent pane spawned or the task executed. No schedule-based firing test at all.
- src-tauri/src/cron/scheduler.rs: 2 unit tests.
- Dreamer: ZERO references in tests/ (`git grep -rn "dreaming|run-worker" tests/` is empty). Only crates/mem-store unit tests for revive_broken and the manual llm-tier evals/dreaming bench. The full pipeline (cron → hidden worker spawn → `mem run-worker dreaming-*` → mem writes → diary) is untested.

## Goal
Full testing catches a dead cron scheduler and a dead dreamer pipeline — the failure mode that silently ate two weeks can no longer pass CI/e2e.

## Scope
1. CRON FIRING e2e (debug clock — `/api/debug/clock`, the cron scheduler already takes an injectable clock): create a cron scheduled 2 minutes out → advance the debug clock past it → assert (a) a run record appears with the SCHEDULED (not manual) origin, (b) the worker pane actually spawned (hidden pane exists with the cron's task, or pane lifecycle events recorded), (c) the task's split-marker output is present (`('CRON_'+'FIRED_<runid>')` typed by the task — echo-proof). Negative: disabled cron + clock advance → NO new run (absence-polling, not sleep-and-hope).
2. CRON TRIGGER hardening: extend test_cron_trigger to assert the run actually executed (worker pane spawned + completed / history entry has a terminal status), not just that a history row exists.
3. DREAMER WORKER e2e (non-llm): on fresh dev, run `codeterm mem run-worker dreaming-normalize --json` against the dev mem db → assert exact `ok:true` shape + worker lifecycle (worker list shows it ran / report recorded). Seed the dev db first via `mem save` so normalize has real input; idempotency: second run also `ok:true`.
4. DREAMER LIVENESS guard: a test (or health surface assert) that the standing dreaming cron EXISTS and is ENABLED on a fresh dev boot — the regression class where the cron itself vanishes/disables. If dev boot doesn't seed dreaming crons by design, document where liveness is supposed to come from and test THAT.
5. All per docs/contributing/writing-tests.md: split markers, exact shapes, absence-polling negatives, missing precondition = FAIL.

## Forbidden
- NO git mutations. NO touching evals/. dev:7686 only inside the granted window (ask the arbiter).
- No weakening the debug-clock plumbing; if the clock can't drive the cron scheduler end-to-end, that's a FINDING (the TTL-xfail lesson) — report it, don't fake it with sleeps.

## Report
Standard format to pane 5c64a651: STATUS / CHANGED / VERIFIED (paste outputs) / NOTES.
