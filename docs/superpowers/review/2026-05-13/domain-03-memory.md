# Domain Review: Memory Subsystem

**Date:** 2026-05-13
**Reviewer:** subagent (claude-sonnet-4-6)
**Scope:** `src-tauri/src/mem/` + `src-tauri/src/api/mem_routes/`

---

## Files Reviewed

43 files across two directories:

```
src-tauri/src/api/mem_routes/calibration.rs
src-tauri/src/api/mem_routes/dream.rs
src-tauri/src/api/mem_routes/graph.rs
src-tauri/src/api/mem_routes/items.rs
src-tauri/src/api/mem_routes/mod.rs
src-tauri/src/api/mem_routes/pinned.rs
src-tauri/src/api/mem_routes/sync.rs
src-tauri/src/api/mem_routes/workers.rs
src-tauri/src/mem/activity.rs
src-tauri/src/mem/auto_extract.rs
src-tauri/src/mem/dreaming.rs
src-tauri/src/mem/dreaming/recluster.rs
src-tauri/src/mem/dreaming/recluster/tests.rs
src-tauri/src/mem/embed_queue.rs
src-tauri/src/mem/embeddings.rs
src-tauri/src/mem/extract.rs
src-tauri/src/mem/file_sync.rs
src-tauri/src/mem/graph.rs
src-tauri/src/mem/ids.rs
src-tauri/src/mem/layers/mod.rs
src-tauri/src/mem/layers/score.rs
src-tauri/src/mem/layers/tests.rs
src-tauri/src/mem/learnings.rs
src-tauri/src/mem/manager_crud.rs
src-tauri/src/mem/manager_graph.rs
src-tauri/src/mem/manager_queries.rs
src-tauri/src/mem/manager_search.rs
src-tauri/src/mem/manager_tests.rs
src-tauri/src/mem/mod.rs
src-tauri/src/mem/reindex.rs
src-tauri/src/mem/rerank.rs
src-tauri/src/mem/search.rs
src-tauri/src/mem/seed.rs
src-tauri/src/mem/store/blob.rs
src-tauri/src/mem/store/crud.rs
src-tauri/src/mem/store/mod.rs
src-tauri/src/mem/store/pinned.rs
src-tauri/src/mem/store/query.rs
src-tauri/src/mem/store/schema.rs
src-tauri/src/mem/store/tests.rs
src-tauri/src/mem/supervisor.rs
src-tauri/src/mem/worker_pool.rs
src-tauri/src/mem/workers.rs
```

---

## Findings

### CRITICAL

#### C-01 — `mem/dreaming.rs`: file grew past the allowlist cap
- **File:** `src-tauri/src/mem/dreaming.rs`
- **Lines:** 1096 (allowlist cap: 1095; rule: must shrink, never grow)
- **Detail:** The file is one line over the explicit `file-size-allowlist.txt` entry. The allowlist comment says "must shrink not grow." Any addition to this file is a CI violation. Split candidate: cron-management helpers (`ensure_dreaming_cron`, `remove_legacy_librarian_cron`, ~50 lines) can move to `dreaming/scheduling.rs`; `prepare_rem_data` + `spawn_rem_worker` to `dreaming/rem.rs`. That alone would drop the file well below 1000 lines.

---

### HIGH

#### H-01 — `mem/rerank.rs`: blocking HTTP on tokio thread
- **File:** `src-tauri/src/mem/rerank.rs`
- **Lines:** `call_anthropic` / `call_openai` use `reqwest::blocking::Client`
- **Detail:** Both provider calls are synchronous (blocking thread sleep inside reqwest). The `Reranker::rerank` trait method is sync, and it is invoked from the `mem_search` async handler without a `spawn_blocking` wrapper when rerank is enabled. A blocking HTTP call (latency: 200ms–5s) on a tokio worker thread starves the async runtime. Fix: wrap the rerank call in `tokio::task::spawn_blocking` inside `mem_search`, or convert the `rerank` trait method to async.

#### H-02 — `mem/graph.rs` `add_alias`: silent data loss on relation rewiring collision
- **File:** `src-tauri/src/mem/graph.rs`, `add_alias` function
- **Detail:** The alias merge does `UPDATE OR IGNORE` to rewire relations to the canonical entity. On unique-constraint conflict, `OR IGNORE` silently drops the update. Immediately after, the code executes `DELETE FROM relations WHERE from_id = ?1 OR to_id = ?1` unconditionally. Any relation that failed the `UPDATE OR IGNORE` (i.e., was not moved) is still deleted. Net result: relations can be permanently removed without being moved to the canonical entity. Fix: use `UPDATE OR REPLACE` (replacing the conflicting row) or collect rewire failures and skip the delete for those rows.

---

### MEDIUM

#### M-01 — `mem/dreaming.rs`: cron scheduling mixed into dreaming module
- **File:** `src-tauri/src/mem/dreaming.rs`
- **Detail:** `ensure_dreaming_cron`, `remove_legacy_librarian_cron`, and `remove_legacy_cron_jobs` are scheduling/lifecycle helpers with no dreaming logic. They account for ~50 lines and have separate concerns from the phase-execution and REM data functions. Extract to `dreaming/scheduling.rs` when shrinking the file to comply with C-01.

#### M-02 — `mem/layers/mod.rs`: over mod.rs advisory limit
- **File:** `src-tauri/src/mem/layers/mod.rs`
- **Lines:** 368 (advisory: 300)
- **Detail:** `render_l1`, `item_line`, `approx_tokens`, `truncate_chars`, `truncate_tokens` are pure-rendering helpers with no query logic. Move to `layers/render.rs`. The `collect_l1_candidates` + `load_items` query logic can stay in `mod.rs`, or move to `layers/candidates.rs`.

#### M-03 — `mem/store/mod.rs`: over mod.rs advisory limit
- **File:** `src-tauri/src/mem/store/mod.rs`
- **Lines:** 367 (advisory: 300)
- **Detail:** The file contains type definitions (`MemKind`, `MemScope`, `NewMemItem`, `MemItem`, `MemUpdate`, `TailEntry`, `GraphNode`, `GraphEdge`, `GraphDump`, `GraphDumpStats`), history types (`HistoryEntry`, `HistorySnapshot`), and the `with_changed_by` / `history_log` / `get_item_history` machinery. Move types to `store/types.rs`; the history functions to `store/history.rs`. `mod.rs` becomes a facade with re-exports only.

#### M-04 — `mem/search.rs`: write-on-every-search, no worker bypass
- **File:** `src-tauri/src/mem/search.rs`, `run_with_config`
- **Detail:** Every FTS hit calls `store::get()` via `get_or_synthesize_entity`, which writes `accessed_at` and `access_count` on every read. The `mem_get` HTTP handler has an `is_worker_call` check that skips this bump; the search path has no equivalent. Worker-driven semantic searches (extraction, dreaming, calibration) inflate access counts and trigger write transactions on every candidate. Fix: thread a `skip_access_bump: bool` flag through `store::get()` and set it in search when the caller is a worker context.

#### M-05 — `mem/workers.rs`: hardcoded 30-minute watchdog ignores config
- **File:** `src-tauri/src/mem/workers.rs`, `spawn_headless`
- **Detail:** The in-process tokio watchdog deadline is `Duration::from_secs(30 * 60)` hardcoded. The cron-created supervisor tasks use the configured `dreaming_timeout_seconds`. If a user sets a shorter timeout (e.g., 10 minutes) in config, the supervisor subprocess times out at 10 minutes but the in-process watchdog fires 20 minutes later, holding state for dead tasks. Fix: pass the configured timeout into `spawn_headless`.

#### M-06 — `mem/graph.rs`: `conn.prepare()` inside BFS loop
- **File:** `src-tauri/src/mem/graph.rs`, `traverse` and `shortest_path`
- **Detail:** Both graph-traversal functions call `conn.prepare(SQL)` on every BFS hop iteration. SQLite statement preparation is O(SQL length) and acquires internal locks. For deep graphs (>20 hops) this is measurably slow. Fix: prepare once before the loop and use `stmt.reset()` / rebind params on each iteration.

#### M-07 — `api/mem_routes/calibration.rs`: abort threshold below query limit
- **File:** `src-tauri/src/api/mem_routes/calibration.rs`
- **Detail:** `CANDIDATE_QUERY_LIMIT = 200` but `CANDIDATE_ABORT_THRESHOLD = 100`. The SQL fetches up to 200 rows; if count > 100 the sweep aborts. The upper half of the query result (rows 101–200) is fetched from disk, deserialized, and then discarded. Either lower `CANDIDATE_QUERY_LIMIT` to match the threshold, or remove the abort and handle the full 200.

#### M-08 — `mem/search.rs`: misleading score-order comment
- **File:** `src-tauri/src/mem/search.rs`, around line 401 / 513
- **Detail:** Comment says "Higher RRF score = more relevant" but `SearchHit.score` is stored as `-adjusted` (negated) and sorted ascending, so lower is better. The negation is an intentional `BinaryHeap` trick, but the comment contradicts it and will confuse anyone reading the sort or the score field on a returned hit. Fix: update the comment to match actual semantics, or store positive scores and sort descending explicitly.

---

### LOW

#### L-01 — `mem/dreaming.rs` tests: `std::mem::forget(dir)` leaks temp dirs
- **File:** `src-tauri/src/mem/dreaming.rs`, test helpers
- **Detail:** Test-local `TempDir` instances are forgotten via `std::mem::forget(dir)` to prevent cleanup during assertions. This leaves OS temp directories behind on every test run. Use `TempDir::into_path()` or keep the binding alive to the end of the test instead of forgetting.

#### L-02 — `mem/dreaming.rs`: cron fires at server-local time (tz: None)
- **File:** `src-tauri/src/mem/dreaming.rs`, `ensure_dreaming_cron`
- **Detail:** The dreaming cron is created with `tz: None`, meaning the cron expression (`0 3 * * *`) fires at the machine's local timezone midnight. On a server deployed in UTC+0 this is fine; on a developer laptop in a non-UTC zone the dreaming window drifts. Low severity since this is a single-user app, but worth documenting or explicitly pinning to UTC.

#### L-03 — `mem/rerank.rs`: production TODO comment
- **File:** `src-tauri/src/mem/rerank.rs`, line ~204
- **Detail:** `// Provider-aware prompts are still TODO.` — a reminder comment in production code. Track as a GitHub issue or remove.

#### L-04 — `mem/store/crud.rs`: write-on-every-read under load
- **File:** `src-tauri/src/mem/store/crud.rs`, `get()`
- **Detail:** `get()` always issues a write transaction to bump `accessed_at` and `access_count`. Under read-heavy workloads (e.g., wake-up loading 50 pinned + 100 ranked items) this serializes 150 write transactions through the single `parking_lot::Mutex<Connection>`. Acceptable for current scale but will bottleneck if item loads increase. Mitigated if M-04 (skip bump for workers) is addressed first.

#### L-05 — `mem/store/schema.rs`: migration pragma checks re-run at every startup
- **File:** `src-tauri/src/mem/store/schema.rs`, `init_schema`
- **Detail:** `PRAGMA table_info(mem_items)` is queried multiple times on every startup to check whether migration columns exist. After migration is complete, these checks are wasted round-trips. Use a `user_version` PRAGMA to gate migrations, or accept the minor startup overhead (columns exist = fast pragma, no actual migration SQL runs).

#### L-06 — `mem/store/query.rs`: `graph_full()` swallows entity/edge errors silently
- **File:** `src-tauri/src/mem/store/query.rs`, `graph_full`
- **Detail:** Entity and edge query failures are caught with `if let Ok(...)` and produce partial data without logging. A malformed schema or locked table during the dump returns a partial graph silently. Add a `log::warn!` at minimum so errors surface in `codeterm.log`.

---

### OK

- `mem/dreaming/recluster.rs` — `run_recluster_phase_with_lock` correctly drops the `parking_lot::Mutex<Connection>` lock between the DB read phase and the async LLM call (lines 141–163), then re-acquires for validation + write. No lock-hold-across-await.
- `api/mem_routes/dream.rs` — Recluster invocation path is correct; async handler calls the lock-aware variant.
- `mem/supervisor.rs` — Circuit breaker (3 failures / 60s window) is clean. `spawn_and_wait` uses `kill_on_drop(true)` subprocess lifecycle correctly.
- `mem/embed_queue.rs` — Bounded `sync_channel(1024)` with explicit drop-on-full is intentional backpressure. Single background thread owns the embed `Connection` — no concurrent access.
- `mem/embeddings.rs` — Mutex held across subprocess stdin/stdout I/O, but all callers are inside `spawn_blocking` or the embed queue thread. No async starvation.
- `mem/auto_extract.rs` — `format_existing_graph_types` acquires `conn().lock()` twice in the same function, releasing between calls. No nested lock violation.
- `mem/worker_pool.rs` — Per-provider `Semaphore` pool. `Mutex<HashMap>` held only long enough to get/insert the `Arc<Semaphore>`. Clean.
- `mem/layers/score.rs` — Static SQL constants (`PASS1_SQL`, `PASS2_SQL`) and pure scoring formula. No issues.
- `mem/search.rs` — FTS filter SQL uses `format!()` only for parameter numbering; values are bound via `params_from_iter`. No SQL injection. `find_graph_boosted_items` UNION reuse of `?NNN` numbered params is correct SQLite semantics.
- `mem/graph.rs` `tunnels` — `format!` used for static `active_clause` strings only (`""` or `" AND r.valid_to IS NULL"`). Safe.
- `api/mem_routes/mod.rs` — `sanitize_actor` prevents audit-trail forgery; `validate_item_fields` is consistently used across save/update.
- `api/mem_routes/sync.rs` — `try_conn_for(500ms)` with 503 fallback is the correct non-blocking pattern.
- `api/mem_routes/items.rs` — `wake_up_budget_below_l0_floor_does_not_underflow` test covers the edge case in `wake_up_options_from_params`.
- All other files (`activity.rs`, `extract.rs`, `file_sync.rs`, `ids.rs`, `learnings.rs`, `manager_*.rs`, `reindex.rs`, `seed.rs`, `store/blob.rs`, `store/pinned.rs`, `store/tests.rs`, `layers/tests.rs`, `manager_tests.rs`, `dreaming/recluster/tests.rs`) — no actionable findings.

---

## Summary Table

| ID | Severity | File | Issue |
|----|----------|------|-------|
| C-01 | CRITICAL | `mem/dreaming.rs` | 1096 lines — 1 past allowlist cap of 1095 |
| H-01 | HIGH | `mem/rerank.rs` | Blocking HTTP (`reqwest::blocking`) on tokio thread via sync `rerank()` trait method |
| H-02 | HIGH | `mem/graph.rs` | `add_alias`: silent data loss — relations skipped by `UPDATE OR IGNORE` still deleted |
| M-01 | MEDIUM | `mem/dreaming.rs` | Cron scheduling helpers mixed into dreaming module — extract to `dreaming/scheduling.rs` |
| M-02 | MEDIUM | `mem/layers/mod.rs` | 368 lines (advisory 300) — render helpers extractable to `layers/render.rs` |
| M-03 | MEDIUM | `mem/store/mod.rs` | 367 lines (advisory 300) — types + history extractable to `store/types.rs` / `store/history.rs` |
| M-04 | MEDIUM | `mem/search.rs` | Write-on-every-search (`accessed_at` bump) with no worker-context bypass, unlike `mem_get` |
| M-05 | MEDIUM | `mem/workers.rs` | Hardcoded 30-minute watchdog ignores `dreaming_timeout_seconds` config |
| M-06 | MEDIUM | `mem/graph.rs` | `conn.prepare()` inside BFS loop in `traverse` and `shortest_path` |
| M-07 | MEDIUM | `api/mem_routes/calibration.rs` | `CANDIDATE_QUERY_LIMIT=200` vs `CANDIDATE_ABORT_THRESHOLD=100` — fetches 2× needed rows |
| M-08 | MEDIUM | `mem/search.rs` | Comment "Higher RRF score = more relevant" contradicts negated score storage |
| L-01 | LOW | `mem/dreaming.rs` | `std::mem::forget(dir)` in tests leaks temp directories |
| L-02 | LOW | `mem/dreaming.rs` | Cron `tz: None` fires at server-local time instead of UTC |
| L-03 | LOW | `mem/rerank.rs` | Production TODO comment — "Provider-aware prompts are still TODO" |
| L-04 | LOW | `mem/store/crud.rs` | Write transaction on every `get()` — unnecessary under worker-driven read bursts |
| L-05 | LOW | `mem/store/schema.rs` | Migration pragma checks re-run at every startup regardless of migration state |
| L-06 | LOW | `mem/store/query.rs` | `graph_full()` swallows entity/edge query errors silently |
