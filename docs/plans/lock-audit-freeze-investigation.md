# Lock Audit — Freeze Investigation

Context: prod CodeTerm occasionally freezes hard on macOS and sometimes on
Windows while navigating Settings. Dev instance rarely shows it. Hypothesis
was deadlock; full audit below.

## Canonical lock order

From `CLAUDE.md` — holding a higher-numbered lock while acquiring a
lower-numbered one is a bug. `parking_lot::Mutex` is NOT reentrant.

1. `mux` (MultiplexerState)
2. `output_buf` (OutputBuffer)
3. `screen_state` (ScreenState)
4. `agent_tracker` (PaneStatusTracker)
5. `pty` (PtyManager)

Proposed addition (see below): `cron_state` sits between #2 and #3, i.e.
always take `cron_state` BEFORE `output_buf`.

## Findings

### Real deadlocks

None confirmed. Every nested-lock site in the codebase acquires locks in a
consistent order; no reverse-order pair exists as of commit `a905d3d`.

Rust guard-lifetime gotcha that produced false positives in the initial
audit: `let x = obj.lock().method();` drops the `MutexGuard` at the `;`.
Only `let g = obj.lock(); ... use g ...` keeps a guard alive across
subsequent statements. Method-chain `lock()` calls do NOT nest with
following statements.

### Consistent nested-lock pairs (must stay in this order)

`cron_state → output_buf` at three sites:

- `src-tauri/src/commands.rs:323` — `read_pty` error path on PTY death.
- `src-tauri/src/service/pty_lifecycle.rs:183` — PTY exit handler.
- `src-tauri/src/service/cron.rs:258` — cron completion path.

All three: take `cron_state` guard, then inside the block read from
`output_buf`. No site does the reverse. Safe today, but fragile. Any future
code that holds `output_buf` and then calls into a cron helper will
deadlock.

Mitigation:

1. Document `cron_state` in `CLAUDE.md` lock order (between #2 and #3).
2. Optional refactor: read `output_buf` text BEFORE acquiring `cron_state`
   at each of the three sites. Removes the nesting entirely.

### Starvation (primary suspect for the observed freeze)

`src-tauri/src/mem/dreaming.rs:37-127` — `light_phase` holds the
`mem.conn()` `Mutex` guard for the entire candidate loop:

```rust
let conn = mem.conn();
let conn = conn.lock();  // held until function returns
let mut stmt = conn.prepare(...)?;
let candidates: Vec<...> = stmt.query_map(...).collect();
for (id, ...) in &candidates {
    // ... scoring ...
    if score >= 0.5 {
        super::store::with_changed_by("dreaming:rem", || {
            conn.execute("UPDATE mem_items ...", ...)?;
            super::store::history_log(&conn, id, "promote", ...);
        });
    }
}
```

Note: no deadlock — `with_changed_by` is a thread-local swap (no lock),
`history_log(conn: &Connection)` takes the already-held guard's reference.

Problem: ANY other caller wanting `mem.conn().lock()` blocks for the whole
loop. On a large memory store this can be seconds. Callers that block
include:

- Settings UI panels that list `mem_items`.
- General Agent bootstrap (`mem wake-up`).
- `mem search`, `mem get`, `mem save` from CLI, REST, Socket.
- Extraction and file-sync workers on their own ticks.

Matches the observed symptom: user clicks around Settings → UI queries mem
→ blocks behind dreaming → whole app appears frozen until the loop
finishes.

Same pattern likely exists in `deep_phase` (not audited line-by-line);
check before fixing only `light_phase`.

### Modules audited and declared clean for deadlock

- `src-tauri/src/mem/**` — no recursive `conn` locks; worker entry points
  drop their guards before calling helpers.
- `src-tauri/src/cron/**` — no multi-lock patterns.
- `src-tauri/src/service/output.rs` — method-chain `mux.lock()` /
  `screen_state.lock()` temporaries drop at `;`; no overlap.
- `src-tauri/src/service/pane.rs` — sequential acquisitions.
- `src-tauri/src/api/**`, `src-tauri/src/dispatch/mod.rs`,
  `src-tauri/src/bridge/**` — `output_buf` guards held only in short
  scopes that do not touch `cron_state`, `mux`, `agent_tracker`.

Perf concern (not a deadlock): `service/output.rs:203-265`
`apply_prompt_decision` re-locks `mux` up to 4 times per invocation. Not
broken, but high-churn path — worth snapshotting pane data once.

## Fix plan

Priority order — start from the top, each step is independent and safe to
ship alone.

1. **`mem/dreaming.rs` — release conn between candidates.** Collect the
   full `candidates` Vec under the lock, drop the guard, then iterate.
   Re-acquire `conn` per-UPDATE (or per small batch). Same change for
   `deep_phase`.
2. **`CLAUDE.md` lock order — add `cron_state`.** Insert between
   `output_buf` (#2) and `screen_state` (#3). Document the three sites as
   the canonical pattern.
3. **Optional: flip order at the 3 nested sites.** Read `output_buf` text
   into a local first, then take `cron_state`. Eliminates the pair
   entirely and makes the added lock-order entry unnecessary.
4. **Dev-only: enable `parking_lot` `deadlock_detection` feature.** Logs
   any pair of locks ever taken in conflicting orders, even without a
   triggering race. Cheap insurance against future regressions.
5. **`apply_prompt_decision` cleanup.** Snapshot pane fields under a
   single `mux` lock, then release. Reduces contention on the hottest
   path in output processing.

## What the symptom is NOT

Not a deadlock at the OS level — nothing mutual-exclusive is pointed at
something else waiting back. The app recovers on its own when a long
dreaming cycle finishes; it does not need a kill. Thread dumps during a
freeze (`sample <pid>` on macOS, ProcDump on Windows) should show one
thread inside `rusqlite` in `dreaming.rs` and N threads parked in
`parking_lot` on `mem.conn()`. That confirms starvation, not deadlock.

## Verification after fix

1. Rebuild prod with fix #1 applied.
2. Manually run `codeterm mem dream now` on a populated DB while clicking
   through Settings. UI must stay responsive.
3. Measure: `dreaming light_phase` candidate count × per-item UPDATE time
   ≈ total blocked time before fix. After fix, UI should never block more
   than a single UPDATE's duration.
4. If freeze still reproduces after #1 — take a thread sample during the
   hang and re-audit. Possible secondary suspects: `file_sync` scans,
   `auto_extract` workers, or a Tauri command taking `mux` across a long
   operation.
