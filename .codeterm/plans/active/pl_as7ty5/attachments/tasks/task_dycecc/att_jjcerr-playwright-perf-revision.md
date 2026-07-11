# Playwright Golden Suite + CodeTerm Performance Revision

Solo static revision by the Planner (user-directed). Sources: tests/playwright/* (config, fixtures, reset, poll, all suite specs), Desktop report `CodeTerm-Playwright-Golden-Test-Plan.md` (183 golden tests / 65 files), src-tauri agent provider surface. No tests were executed.

## A. Where golden wall-clock actually goes (183 tests, workers:1, serial)

Ranked by measured/structural impact:

1. **WHALE 1 — real-LLM agent spawns (~25+ tests).** 03-agents (4 tests, caps 300–420s), cross-pane-chat (420s), 20-new-session picker (~14 tests x 180s, each pair-spawn materializes a REAL orchestrator+planner incl. LLM greeting), 12-web live chats, 10-restore matrix, 07-cron oneshot real run. Even when green these burn minutes of real LLM latency each. Estimated 60–80% of total suite wall-clock. `src-tauri/src/agent/test_provider.rs::MockProvider` is `#[cfg(test)]`-only — the live daemon has NO stub provider, so Playwright cannot avoid real Claude today.
2. **WHALE 2 — daemon kill/restart cycles.** 02-multiplexer (kill/restart), 15-extensions, 18-reorder, 10-restore. Each cycle = daemon down + `ensureDevConsole()` re-adopt (up to minutes on cold path). Also poisons any future parallelism: a restart test kills the daemon under every other shard.
3. **WHALE 3 — `workers: 1` structural ceiling.** All suites share ONE daemon + UI focus + mux state (config comment). No parallelism is possible without instance isolation. `CODETERM_INSTANCE`/port mechanism already exists for worktrees — unused by the test harness.
4. **TAX 4 — `apiReset` after EVERY test.** `helpers/fixtures.ts` api fixture teardown sweeps all workspaces + all panes over REST x183 tests, even for tests that created nothing. `paneCleanup` already tracks created panes — the blanket sweep is redundant most of the time.
5. **TAX 5 — slow polling.** `pollMarkerInDom` polls at 1000ms with default timeout 240s; `pollUntil` backs off 250ms -> 2000ms. On 100+ marker waits, late detection adds tens of seconds.
6. **TAX 6 — 88 fixed sleeps** across suites/helpers (mostly 100–500ms, outliers 1s/1.5s/2.2s; hotspots: canvas-inspect.ts x11, pin-general x8, tab-switch-latency x7, canvas-perf x6). ~30–45s direct waste + charter violation (condition-waits, never sleeps).
7. **Stale meta:** AUDIT.md dates June (78 tests) vs current 183/65 files; static-audit tooling exists but has not been re-run.

## B. Test-suite speedup plan (target: golden shard minutes, not hours)

1. **Runtime stub agent provider** (dev-build only, env/debug-route gated): scripted deterministic replies + tool-call transcripts, instant readiness. Convert agent-flow tests that verify UI/plumbing (picker spawns, chat rendering, markers, restore identity) to the stub. Keep 3–5 true end-to-end LLM tests in a separate `@live` shard, run less frequently. Expected: minutes -> seconds on ~25 tests; biggest single win.
2. **Restart-isolation shard:** move all daemon-kill/restart tests into one dedicated serial shard at the END; all other shards then never lose the daemon (precondition for parallelism, and removes mid-suite re-adopt storms).
3. **Multi-instance parallel sharding on one machine:** N dev daemons via `CODETERM_INSTANCE=<n>`/distinct ports, one Playwright worker pinned per instance. Focus/mux races disappear because each worker owns its instance. Combined with cross-machine sharding (ROG/eight/ubunta) this multiplies throughput.
4. **Diff-based reset:** extend `paneCleanup`-style tracking to workspaces; teardown closes only what the test created; keep the blanket `apiReset` as suite-boundary (per-file) safety net, not per-test.
5. **Kill fixed sleeps (88 sites) + tighten polling:** condition-waits per charter; `pollMarkerInDom` polling 1000 -> 250ms; `pollUntil` cap backoff at 1000ms. Cheap mechanical pass, immediately shaves minutes and removes flake.
6. **Budget right-sizing after stub conversion:** 420s/300s caps drop to 60–90s for converted tests; per-shard `--global-timeout` recomputed (shard table from task_rhbf8k).
7. **Refresh static audit** (AUDIT.md tooling) against 183 tests as the hygiene gate for the above.

## C. CodeTerm app performance plan ("must fly, zero freezes") — measured by Playwright

Existing measured budgets to build on (already in tree):
- keystroke->render p95 <= 32ms / max <= 100ms under 25-tab load (16-chat-view/composer-output-performance, desktop + web variants)
- sidebar tab-switch p95 <= 150ms (13-canvas/tab-switch-latency)
- hidden panes: <= 3 re-renders + longtask < 500ms under agent_state churn (12-web/multitab-perf)
- canvas frame stats + PerformanceObserver longtask harness (helpers/canvas-perf.ts)

Plan: consolidate into a dedicated **`perf` Playwright project** — fast, deterministic, zero LLM (stub provider), run per-push on ROG (desktop/CDP) + eight (web-mode):
1. Promote the four existing budget tests into the `perf` project with hard asserts (regression gate).
2. Add missing metrics: PTY write->render replay latency + frame-bounded slicing assert (guards commits 08bfafaff/6bef4c8dc); long-history open lands at bottom w/o burst scroll (task_w6t5pq contract); cold open -> interactive; IPC starvation guard (trivial `get_config` p95 while churn runs — catches async-runtime starvation class).
3. Every run emits a JSON metrics artifact (p50/p95/max per metric) for trend tracking across builds; budgets are asserts, trends are data.
4. Wire into the plan QA gates: `perf` project green required before integration merges to feat/1.7.2-stability.

## D. Sequencing

Wave A (parallel): stub provider (Rust impl) | sleep purge + polling tighten (mechanical) | restart-shard regroup + diff reset.
Wave B (after stub): convert agent-flow tests; right-size budgets; refresh audit.
Wave C: multi-instance parallel sharding (needs restart isolation done).
Wave D: `perf` project consolidation + new metrics + gate wiring (can start parallel to B).
