# Restart, restore and session continuity

Purpose: prove saved state returns to the same session and work, with no accidental thaw/replay. Core: retained idle tab and task-bound Worker through an owner-controlled restart.

## Preconditions / fixtures

Canonical owner grants a specific restart window, build and instance. Shared DEV7686 is never stopped by a tester; prod7685 is never killed. An isolated named-instance bench can own its lifecycle only under its own explicit grant. Read dev-run/build/testing skills and `evals/core-journeys/README.md`. Do not copy older restore Playwright instructions or bypass its safety guards.

Create disposable idle Codex and Claude tabs plus a tiny dispatched Worker in a fixture plan. Before shutdown record health/boot/listener PID+kernel start, tab/session/backing/provider, task/attempt/binding nonce, exact marker turn IDs, mux state and selection. Require the marker persisted in the provider's own bound-session store, not just UI or newest cwd file. A missing pre-restart record is setup failure, not a restore result.

## Steps / expected evidence

1. Save `dev_get /api/mux/state`, `dc tab inspect "$TAB" --json`, `dc tab chat --tab "$TAB" --json` and `dc sessions get "$SESSION" --json`. Use only metadata's exact persisted path for any store read. Do not scrape global/latest sessions.
2. Stop/long-turn subcase: wait on the owned working event, then owner controls shutdown or an explicit agent Stop. A busy-at-shutdown fixture needs its own grant; idle-only results cannot cover it. Preserve input count and task state.
3. Owner performs the named canonical lifecycle operation from the matching skill in its Team utility shell. Tester waits on a real readiness/boot change, verifies expected build and changed process identity, and reads mux BEFORE clicking any tab. Elapsed time is not a restart.
4. Idle retained tabs must remain frozen until their documented selection/input trigger; a selected tab may legitimately resume. Record frozen field, not a vague live flag. Visit an owned tab only after the pre-visit snapshot. Verify same tab/session/provider/backing, no cross-tab history and original marker once.
5. For the Worker require same plan/task/current attempt/session (new nonce may be expected); then execute its minimal report from the restored Worker and prove parent receipt/task attribution. Missing attempt or report refusal is FAIL even when the tab looks fine.
6. Send one fresh opaque marker to an owned restored tab using --from-pad. Require own-session turn once, expected final assistant and correct DOM. Never resend written/uncertain input. For Stop, require the old interrupted turn remains interrupted and old input never replays after view reload/restart.
7. If memory/resources are scoped, sample the exact listener's process tree before/after with platform-native read-only tools; record units, descendants and timestamps. A single measurement cannot pass a long-running leak claim.

PASS requires stable bindings, durable history and actual resumed operation. FAIL: lost/foreign session, new-thread substitution, replay, premature thaw or stale task attempt/report. NOT RUN: no restart grant, no persisted marker, busy fixture absent or unsupported provider. Source repair is not live restore proof.

Cleanup owned fixtures under the owner's plan-review/lane rules after final evidence. Restore the owner's dev lifecycle if the grant requires it; do not restart merely to tidy.

Seeds: Keel critical idle freeze/bound resume/input-once PASS; Turing reports Claude interrupted/restart/reload, Worker description/model binding PASS. Busy-at-shutdown, failure-start report and stress stayed NOT RUN. Turing observed restored Open transcript409; retain that as an open failure until specifically reverified.

Automation: `evals/core-journeys` J1–J4 is the named-instance authoritative combined oracle with its own guard; run only the owner-authorized exact command on a disposable instance. `evals/upgrade-restore` adds old-version/large-state inputs. Do not run those builds/restarts as an incidental manual check.
