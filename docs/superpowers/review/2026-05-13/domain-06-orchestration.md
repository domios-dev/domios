# Domain Review: Orchestration Core

## Files Reviewed

- `src-tauri/src/dispatch/mod.rs`
- `src-tauri/src/multiplexer/mod.rs`
- `src-tauri/src/multiplexer/commands.rs`
- `src-tauri/src/multiplexer/types.rs`
- `src-tauri/src/multiplexer/state/mod.rs`
- `src-tauri/src/multiplexer/state/alias.rs`
- `src-tauri/src/multiplexer/state/names.rs`
- `src-tauri/src/multiplexer/state/pane.rs`
- `src-tauri/src/multiplexer/state/role.rs`
- `src-tauri/src/multiplexer/state/snapshot.rs`
- `src-tauri/src/multiplexer/state/tab.rs`
- `src-tauri/src/multiplexer/state/tests.rs`
- `src-tauri/src/multiplexer/state/workspace.rs`
- `src-tauri/src/service/mod.rs`
- `src-tauri/src/service/agent_restart.rs`
- `src-tauri/src/service/cron.rs`
- `src-tauri/src/service/general_agent/instructions.rs`
- `src-tauri/src/service/general_agent/lifecycle.rs`
- `src-tauri/src/service/general_agent/mod.rs`
- `src-tauri/src/service/general_agent/watchdog.rs`
- `src-tauri/src/service/general_agent/workspace.rs`
- `src-tauri/src/service/hierarchy.rs`
- `src-tauri/src/service/inject_verify.rs`
- `src-tauri/src/service/input_parser.rs`
- `src-tauri/src/service/notification.rs`
- `src-tauri/src/service/orchestrator.rs`
- `src-tauri/src/service/output.rs`
- `src-tauri/src/service/pane.rs`
- `src-tauri/src/service/pty_io.rs`
- `src-tauri/src/service/pty_lifecycle.rs`
- `src-tauri/src/service/spawn.rs`
- `src-tauri/src/service/spawn_tests.rs` (not read — test file only)
- `src-tauri/src/service/sync_watcher.rs`
- `src-tauri/src/service/tests.rs`
- `src-tauri/src/service/workspace.rs`

---

## Findings

### src-tauri/src/service/pane.rs

#### HIGH
- **`close_pane` acquires `mux` three separate times in sequence** (lines 14–16): `pre_focus`, then `close_pane`, then `post_focus`. Between the first and second acquisitions, another thread could close additional panes or change focus, making `pre_focus` stale. The state diff (pre vs. post) is therefore unreliable — a focus-change event could be emitted when focus didn't actually change, or suppressed when it did. Same pattern in `workspace.rs::close_workspace` and `workspace.rs::close_tab`. This is a TOCTOU issue, not a deadlock, but it is an incorrect state comparison.

- **`respond_prompt` (lines 335–388) acquires `mux` to read `pending_prompt`, drops it, then calls `prompt_detector.lock()`, then later acquires `mux` again** (line 364: `self.mux.lock().get_pane(pane_id)`) to read `agent_type`. The second `mux.lock()` is issued after `prompt_detector.lock()` was already dropped, so no concurrent lock ordering issue there. However, the function also does `output_buf.lock()` at line 354 immediately after releasing `mux`. The sequence is: `mux` → (drop) → `output_buf` → (drop) → `prompt_detector` → (drop) → `mux`. No overlapping holds; this is safe. (Was investigated — no violation confirmed.)

#### MEDIUM
- **`rename_pane` (line 28) emits `emit_pane_title_pinned` AND `emit_state_changed`**: both spawn `std::thread::spawn` callbacks. This means for every pane rename there are two detached threads firing. Minor overhead, no correctness issue.

- **`write_pane_agent_type` (line 88) triggers `kick_off_session_detection` which spawns a thread polling for up to 15 minutes** (max_attempts=300, step=3s). Each call to `set_pane_agent_type` or `set_and_lock_pane_agent_type` with a new agent type launches a new 15-minute poller. If an agent type transitions multiple times (e.g. during watchdog restart cycles), stale pollers accumulate. The pollers do self-exit when they observe `p.agent_type != agent_id`, but the polling loop exits only on the next 3-second sleep tick, not immediately. With frequent restarts this could have O(restarts) active threads sleeping simultaneously.

---

### src-tauri/src/service/workspace.rs

#### HIGH
- **`create_workspace` (lines 12–35) acquires `mux` twice with logic between the two acquisitions**. At line 13 it locks `mux` to call `find_workspace_info_by_cwd`, releases it, then at line 15 acquires again for `set_active_workspace`, releases, calls `create_tab_with_activate`, then at line 29 acquires again to `create_workspace`. In the gap between the first lock (existence check) and the second (set_active), a concurrent call could create the same workspace, producing duplicates. The existing dedup logic is correct for non-concurrent callers but is not protected against concurrent creation of the same cwd-named workspace.

#### MEDIUM
- **`close_workspace` / `close_tab` / `close_pane` all use the triple-lock TOCTOU pattern** described in `pane.rs` above (pre-focus lock, mutation lock, post-focus lock as separate acquisitions). Consolidating the mutation + post-focus read into one critical section would eliminate the race.

---

### src-tauri/src/service/orchestrator.rs

#### CRITICAL
- **`spawn_agent` (line 363) acquires `mux` AFTER `set_pane_role` returns**. At line 363, `set_pane_role` is called (which internally locks `mux` via `service/hierarchy.rs::set_pane_role`), and then at line 363b (line 363–368), `self.mux.lock()` is acquired again to set `auto_approve`. These are sequential, non-overlapping acquisitions. No deadlock here. However:

- **`spawn_agent` (lines 303–314) acquires `mux`, reads workspace id/cwd, drops it, then later at lines 321–348 acquires `mux` again to `create_tab`**. Between these two lock acquisitions the workspace could be deleted by a concurrent `close_workspace`. The `create_tab` call will then return `Err("Workspace not found")` which is propagated correctly — this is handled. But pane metadata (cwd, report_target_pane_id, spawned_by_pane_id) is set inside the second lock, then `set_pane_role` runs as a third lock. If role assignment fails (line 359), the pane was already created and mutated but has no role — this partial state is visible and never cleaned up. **Spawn failure mid-way leaves an orphaned pane in the mux.**

- **`launch_agent_in_background` (line 438) does `svc.mux.lock()` while already potentially inside a `svc.pty.lock()` closure at line 450**: the sequence is `pty.lock().spawn(...)` (line 448), then immediately `svc.mux.lock().link_pty(...)` (line 450). These are sequential (pty lock is dropped before mux is acquired), but on the reader-spawning path at line 454, `svc_arc.clone()` is handed into a nested `std::thread::spawn` which calls `start_backend_reader`, which acquires `svc.pty.lock()` first and then `svc.mux.lock()` inside `handle_pty_death`. The lock ordering in `handle_pty_death` is: `agent_tracker` → `mux`. The CLAUDE.md mandated order is mux(1) → agent_tracker(4). **`handle_pty_death` in `pty_lifecycle.rs` acquires `agent_tracker.lock()` before `mux.lock()` — this is a lock ordering violation relative to the mandated order.**

#### HIGH
- **Orphaned pane on partial spawn failure**: In `spawn_agent` (orchestrator.rs), if `set_pane_role` (line 356) fails, the already-created tab and pane remain in the mux with no role, no agent type yet (agent type is set after role, line 380), and no PTY (PTY is launched async). The caller gets `Err(...)` from `spawn_agent` and has no way to clean up the dangling pane. The same pattern applies in `spawn_visible_agent_impl` in `spawn.rs` (line 271 role failure path).

- **`batch_spawn` (orchestrator.rs line 58) calls `spawn_agent` which returns `SpawnResult` with `pty_id: String::new()`** because the PTY is assigned asynchronously. Batch callers that depend on `pty_id` from the result will get an empty string. This is documented but could silently break downstream batch operations that try to use the pty_id.

#### MEDIUM
- **`orchestrator.rs` at 835 lines has at least four distinct responsibilities**:
  1. Batch command dispatch (`batch` + all `batch_*` methods, ~128 lines)
  2. `SpawnAgentParams` / `SpawnResult` data types (~70 lines)
  3. The async `spawn_agent` high-level agent spawn path (~200 lines)
  4. The sync `spawn_agent_sync` / `spawn_agent_sync_with_ready` wrappers (~30 lines)
  5. `launch_agent_in_background` — background thread orchestration for PTY wait + readiness + task delivery (~155 lines)
  6. `build_agent_command` — command string construction (~40 lines + tests)

  Responsibilities 1, 2, 3+4+5, and 6 are each extractable. The file should be split into at minimum: `batch.rs`, `spawn_types.rs`, `spawn_agent.rs`, and `agent_command.rs`.

- **DRY violation — workspace resolution duplicated in three places**: The block `from_parent → workspace_id → active_workspace → first workspace` appears identically in `orchestrator.rs::spawn_agent` (lines 305–313), `spawn.rs::spawn_visible_agent_impl` (lines 211–219), `spawn.rs::spawn_visible_shell_impl` (lines 339–348), and `spawn.rs::spawn_hidden_agent_impl` (lines 498–505). Four copies of the same 8-line resolution logic.

- **DRY violation — env_vars block for hidden spawns duplicated**: The block constructing `CODETERM_PANE_ID`, `CODETERM_SOCKET_PATH`, `CODETERM_API_URL`, `CODETERM_API_TOKEN` from `api_cfg` appears in `spawn.rs::spawn_hidden_shell_impl` (lines 414–433) and `spawn.rs::spawn_hidden_agent_impl` (lines 539–560), and also in `launch_agent_in_background` (lines 441–447). Three copies of the same env-var construction.

- **Role suffix appended to task in two places**: The `match agent.role.as_deref()` block that appends `WORKER_SUFFIX` / `LEAD_SUFFIX` / `ORCHESTRATOR_SUFFIX` appears in `orchestrator.rs::spawn_agent` (lines 389–396) and `spawn.rs::spawn_visible_agent_impl` (lines 292–306). Both blocks are nearly identical.

---

### src-tauri/src/service/spawn.rs

#### CRITICAL
- **`spawn_hidden_agent_impl` (lines 635–692) spawns a nested thread inside a thread**. The outer thread (line 635) immediately spawns an inner thread (line 638) for the backend reader, then calls `poll_shell_prompt` on `screen_state`. The outer thread holds no locks. However, at line 679 in the outer thread, `svc_outer.resolve_pty_id(&pane_id_outer)` calls `self.mux.lock()` — this is fine (sequential). No lock ordering issue here. But the nested `std::thread::spawn` → tokio runtime → `start_backend_reader` → `svc.pty.lock()` then later `svc.agent_tracker.lock()` → `svc.mux.lock()` (in `handle_pty_death`) reproduces the **agent_tracker before mux** ordering violation identified above.

#### HIGH
- **`spawn_hidden_shell_impl` (lines 381–470): PTY is spawned and linked (line 439: `self.mux.lock().link_pty(...)`) before the backend reader is started**. If `start_backend_reader` fails (line 453–457 error path), the PTY process is running, linked to the pane, but no one is reading its output. The PTY will accumulate buffered data until the OS pipe fills. There is no cleanup: the PTY is not killed, the pane is not removed. A failed backend reader leaves the system in a state where the pane appears live but is deaf.

- **`spawn_hidden_agent_impl` (lines 472–702): same issue as `spawn_hidden_shell_impl`** — PTY spawned and linked at line 572, backend reader started inside a detached thread (line 635). If the backend reader spawn fails, the PTY runs silently. Additionally the run_watcher is registered (line 601) and will eventually timeout, but the error-path cleanup for the pane itself is missing.

#### MEDIUM
- **`spawn.rs` at 707 lines** mixes: visible agent spawn, hidden agent spawn, visible shell spawn, hidden shell spawn, `SpawnPaneSpec` / `PaneVisibility` / `PaneCommand` / `AgentSpawnSpec` / `PaneHandle` data types, and `SpawnPaneSpec::from_agent_params` / `to_agent_params` conversions. The four spawn implementations plus data types are extractable into separate files.

- **`spawn_visible_agent_impl` (line 256) calls `mux.rename_pane` inside the mux lock** but `spawn_agent` in `orchestrator.rs` calls `self.rename_pane` (a service-level method that takes its own lock) outside the mux lock. This inconsistency means rename in the `spawn.rs` path bypasses `emit_pane_title_pinned` (since it calls the mux method directly, not the service wrapper), while the orchestrator path correctly fires the title-pinned event.

---

### src-tauri/src/service/pty_lifecycle.rs

#### CRITICAL
- **`handle_pty_death` (lines 165–204) acquires `agent_tracker` then `mux`**:
  - Line 166: `svc.agent_tracker.lock().set_dead(pty_id)` — acquires `agent_tracker` (lock 4).
  - Line 167: `svc.mux.lock().pane_id_for_pty(pty_id)` — acquires `mux` (lock 1).
  - Mandated order: mux(1) → agent_tracker(4). **This is inverted — `agent_tracker` is acquired before `mux`. This is a lock ordering violation.**

  Additionally, line 185 acquires `output_buf` while `mux` is NOT held (safe), but then lines 183–202 acquire `cron_state` which is a `Mutex<CronStore>` — not in the mandated ordering. If any other path acquires cron_state then mux (or agent_tracker), deadlock is possible.

#### HIGH
- **Backend reader in `backend_read_loop` (lines 60–162) acquires `output_buf`, `screen_state` inside `spawn_blocking`** (lines 84–85) and later `mux` outside the task (line 131). Since `spawn_blocking` runs on a thread pool, and the main loop acquires `mux` on a tokio task thread, there is no overlapping hold — but the sequence within a single loop iteration is: `output_buf` (inside spawn_blocking, released on task completion) → `screen_state` (inside spawn_blocking, released) → then `mux` (outside, line 131). The mandated order for output_buf(2) → screen_state(3) is respected within spawn_blocking. `mux` is acquired after both are released. This is safe.

- **`start_backend_reader` (lines 27–57) acquires `svc.pty.lock()` at line 33** and holds it while calling `manager.get_session(pty_id)`, `session.pair.lock().unwrap()` (a `std::sync::Mutex` — panics on poison), and `manager.setup_backend_channel`. The `pair.lock().unwrap()` will **panic if the mutex is poisoned** (e.g. if a thread panicked while holding it). Using `unwrap()` on a `std::sync::Mutex` guard in production code is an `unwrap` in production code finding.

#### MEDIUM
- **`backend_read_loop` does not remove the PTY from the manager on EOF**. `handle_pty_death` marks the agent dead and emits events but does not call `pty.remove()`. PTY entries accumulate in `PtyManager` for dead processes until the pane is explicitly closed. This is a resource retention issue (file descriptors, memory).

---

### src-tauri/src/service/output.rs

#### MEDIUM
- **`process_pty_output` (lines 25–166) acquires `mux`, then `agent_tracker`, then `screen_state`, then `agent_tracker` again** across multiple sequential (non-overlapping) lock takes. The sequence per call:
  1. Line 33: `mux.lock()` → release
  2. Line 45: `screen_state.lock()` → release
  3. Line 47: `agent_tracker.lock()` (get + update) → release
  4. Line 56: `agent_tracker.lock()` again (get_status) → release
  5. Line 79: `mux.lock()` again (get_pane for grace period check) → release
  6. Line 86: `screen_state.lock()` again → release
  7. Line 95: `mux.lock()` again → release
  8. Line 122: `mux.lock()` → release
  9. Line 131: `screen_state.lock()` → release
  10. Line 136 (prompt_detector): `prompt_detector.lock()` — outside mandated order

  None of these overlap — they are all sequential. No deadlock. But this function takes 8+ short-lived locks per PTY chunk. The excessive lock churn makes each lock acquisition a potential contention point as PTY throughput scales. This is a design concern, not a correctness defect.

- **`EVENT_COOLDOWN` uses `std::sync::Mutex` (line 19) with `.lock().unwrap()`** (line 259). If any thread panics while holding this lock, all subsequent calls to `handle_agent_event` will panic permanently. Production code should use `lock().unwrap_or_else(|e| e.into_inner())` or `parking_lot::Mutex`.

- **`apply_prompt_decision` (lines 168–248) acquires `mux` twice in the `NewPrompt`/`PromptUpdated` branch**: once at line 175 to read `auto_approve`, and again at line 215 to write `pending_prompt`. Between these two acquisitions the pane could be closed. The second `get_pane_mut` returning `None` is silently swallowed (no error). Safe but could silently miss the prompt update if pane is closed mid-event.

---

### src-tauri/src/service/hierarchy.rs

#### MEDIUM
- **`list_agents` (line 146) acquires `mux` then `agent_tracker` in that order** — matches the mandated order (mux=1, agent_tracker=4). Clean.

- **`get_hierarchy` (line 174) acquires `mux` then `agent_tracker`** — same pattern, correct.

- **`get_pane_status` (lines 117–144) acquires `mux` (line 118), reads `pane.pty_id`, then acquires `agent_tracker` (line 124)** while `mux` is still held. Lock ordering: mux(1) → agent_tracker(4). This is **correct per the mandated order** — no violation.

- **`submit_report` (lines 46–97) acquires `mux` (line 49), drops it (implicit at end of block), then `report_store.lock()` (line 57)**. The `report_store` is not in the mandated lock ordering; it appears to be an independent resource. No ordering issue detected. However, the `send_text` call on line 89 internally calls `await_pty_link_blocking` which acquires `mux` in a blocking retry loop. This is issued from a sync context (no async here) — blocking is fine, no deadlock.

---

### src-tauri/src/service/general_agent/lifecycle.rs

#### HIGH
- **`start_general_agent` (lines 75–85) acquires `mux` inside an `if let` block, then immediately acquires it again at line 75** (outer `mux.lock()`) and line 76 (`mux.lock().get_pane_mut`). The pattern at lines 75–85 and 91–95 is:
  ```
  let m = mux.lock();      // lock 1
  ...
  drop(m);                 // explicit
  ...
  if let Some(pane) = mux.lock().get_pane_mut(&pid) {  // lock 2
  ```
  The explicit `drop(m)` at line 118 (`drop(m)`) then re-acquires at line 127: `svc.set_agent_session_id(...)` which internally acquires `mux` again. These are all sequential, fine.

- **`start_general_agent` line 75: acquires `mux` (`m`), then at line 73 calls `pty.lock().get_session()`**: this is within the `if let (Some(pid), Some(current_agent))` guard and the `mux` lock `m` is still held. The sequence is `mux.lock()` then `pty.lock()`. The mandated ordering ends at `pty` as position 5 — mux(1) before pty(5) is **correct**. No violation.

- **`restart_general_agent` (lines 349–393) acquires `mux` in the outer block (line 362: `let m = mux.lock()`), reads the pane, then calls `pty.lock().kill()`** at line 365 — `mux` is still held at this point. Then `drop(m)` is explicit (line 369). Then at line 372, `mux.lock().get_pane_mut()` is a re-acquisition after the drop. The sequence while both locks are held is `mux` → `pty` (lines 362–366) — correct per mandated order (mux=1, pty=5).

#### MEDIUM
- **`start_general_agent` lines 141–163: the fresh-session-detection block** (when session_id is invalid) calls `mux.lock().other_session_ids()` then `detect_agent_session_id` (filesystem scan) then `svc.set_agent_session_id()` (which acquires mux again). The filesystem scan happens without holding mux — correct. But there is no atomic claim here: two concurrent start_general_agent calls could both detect the same fresh session and both try to set it. `set_agent_session_id` in `pane.rs` has a conflict guard, so the second write will be rejected and logged — this is handled correctly.

---

### src-tauri/src/service/general_agent/watchdog.rs

#### MEDIUM
- **`needs_restart_decision` (lines 5–27) hardcodes provider-specific strings** ("Claude Code", "? for shortcuts", "Cogitated for", "Brewed for"). This is the architecture note from CLAUDE.md: "Detection uses structural fingerprints, never literal strings". These literals tie the watchdog to claude-specific output and will silently fail for codex/opencode providers. The watchdog also uses the same shell-prompt heuristics as `process_pty_output` output.rs — this duplicates detection logic. Both should delegate to `agent_registry.detect_from_output()`.

- **Watchdog `start_watchdog` sleeps 20 seconds before first check** (line 46) and then 10 seconds between checks (line 50 in the loop). During that 20-second window after app start, a failed GA launch is not retried. This is a known tradeoff (comment at line 45) but means 20–30s of dead GA in the worst case. Not a bug, but a design concern.

---

### src-tauri/src/service/pty_io.rs

#### MEDIUM
- **`send_text_to_pty` (lines 89–119) spawns a thread to send `\r` after 500ms**. The thread holds a clone of `self.pty` (an `Arc<Mutex<PtyManager>>`). If the pane/PTY is closed within 500ms, the deferred write to the now-dead pty_id will fail silently (`let _ = pty.lock().write(...)`). This is intentional (fire-and-forget), but the pty_id string is captured by value, not re-resolved — so there is no risk of writing to a reallocated pane. Acceptable design.

- **`wait_for_pty` (lines 338–356) acquires `mux.lock()` inside a `tokio::time::timeout` async block** (line 344). The `mux` is `Arc<parking_lot::Mutex<_>>`. Parking_lot's `Mutex::lock()` is a blocking call — calling it inside an async task will block the tokio thread. This should use `tokio::task::spawn_blocking` or `tokio::sync::Mutex`. **Blocking a tokio thread in async context is a correctness concern under load.**

- **`send_text_batch` (lines 292–309) calls `tokio::spawn` and acquires `pty.lock()` inside an async task** (line 305). Same issue as above — `parking_lot::Mutex::lock()` blocks a tokio thread.

---

### src-tauri/src/service/pane.rs (continued)

#### MEDIUM
- **`kick_off_session_detection` (lines 167–268) spawns a thread that retries for up to 15 minutes** (300 * 3s). Inside each iteration, it acquires `mux.lock()` (line 191) to read pane state, drops it, then calls `detect_session_id` (filesystem, no lock held), then acquires `mux.lock()` again (line 223) for the atomic claim. The ordering is always `mux`-only; no other lock is held concurrently. Safe, but the 15-minute cap with 3s steps means up to 300 concurrent active threads per agent launch if the system is busy. There is no global limit on the number of detection threads.

---

### src-tauri/src/dispatch/mod.rs

#### MEDIUM
- **`dispatch/mod.rs` at 609 lines contains all command implementations** inline as private functions: `cmd_health`, `cmd_pane_list`, `cmd_send_text`, `cmd_get_output`, `cmd_get_text`, `cmd_pane_close`, `cmd_pane_focus`, `cmd_pane_rename`, `cmd_workspace_create`, `cmd_workspace_rename`, `cmd_workspace_close`, `cmd_workspace_list`, `cmd_pty_resize`, `cmd_pty_kill`, `cmd_list_agent_providers`, plus `next_char_boundary`, plus the full `Command` and `CommandResult` enum definitions, plus extensive inline tests.

  Per CLAUDE.md: `mod.rs` advisory target is <300 lines, and logic should move to concrete files. The command implementations are substantial business logic (ANSI stripping, byte-offset calculation, workspace creation) that belongs in a `commands/` subdirectory. The `Command` enum and `CommandResult` types could go in `commands/types.rs`, with individual command groups in `commands/pane.rs`, `commands/workspace.rs`, etc.

- **`cmd_pane_list` (line 170) uses `unwrap_or_default()` on `serde_json::to_value(p)`** which silently drops a pane from the listing if serialization fails. While Pane serialization is unlikely to fail (all fields are JSON-safe), this masks potential issues.

- **`cmd_get_output` (lines 190–231): `output_buf.lock()` is acquired at line 199 and held while all the text slicing and ANSI stripping happens** (lines 200–224). ANSI stripping via `crate::pty::ansi::clean_terminal_output` could be CPU-intensive on large buffers and extends the time `output_buf` is held. The text should be cloned out of the lock first.

#### LOW
- **`cmd_get_output` acquires `output_buf` directly** (lock 2) without first acquiring `mux` (lock 1). This is a standalone use — `mux` is not needed for this path — so there is no ordering issue. But it skips the lock ordering entirely because only one lock is used. Fine.

---

### src-tauri/src/multiplexer/state/pane.rs

#### MEDIUM
- **`close_pane` (lines 242–275) calls `self.close_pane(&cid)` recursively** for children (line 269). This walks the child hierarchy with the mux's `&mut self` held (since `close_pane` takes `&mut self`). The recursive close path removes children from `self.panes` during iteration. `get_children_ids` (called at line 252) collects IDs first, so the iteration is safe. But deeply nested hierarchies (Orchestrator → Lead → Worker → SubWorker) will have recursion depth = hierarchy depth. No stack overflow risk in practice (max depth is ~4 per CLAUDE.md), but worth noting if depth bounds are ever relaxed.

- **`close_pane` (line 260) calls `self.remove_pane_from_tree(pane_id)` which returns `Err` if the pane is not in any split tree** (e.g. hidden worker pane). The error is propagated as `Result<Vec<String>, String>`. A hidden pane that is not in any split tree (pane registered but `hidden=true`, tab may not exist or may not contain it) would fail to close cleanly. Since hidden panes are created with a tab, this should not happen in practice, but the error path is not tested.

#### LOW
- **`create_workspace` in `workspace.rs` (line 19) sets `tab_count: 1` in the returned `WorkspaceInfo`** even though the workspace has zero tabs (line 11: `tabs: vec![]`). The tab_count field is a lie — the workspace is empty. This is a display-only inconsistency.

---

### src-tauri/src/multiplexer/state/snapshot.rs

#### MEDIUM
- **`to_session_snapshot` (line 93) uses `unwrap_or_default()`** on `SystemTime::now().duration_since(UNIX_EPOCH)` — this cannot fail unless the system clock is before the Unix epoch, so it is safe in practice. But the style convention (CLAUDE.md: "no `unwrap()` in production code") applies equally to `unwrap_or_default()` when it silently swallows an impossible error. Low risk.

- **`from_session_snapshot` (lines 107–216) contains a large deeply-nested closure** `snapshot_to_split` defined inline. At 90+ lines, this inner function should be extracted as a standalone function or method for readability and testability.

---

### src-tauri/src/multiplexer/state/role.rs

#### MEDIUM
- **`set_pane_role` uses `.unwrap()` on `self.panes.get(pane_id)` and `self.panes.get_mut(pane_id)` at lines 21–25, 42, and 60**. These come immediately after `self.panes.contains_key(pane_id)` checks, making the unwraps logically safe (no one else can remove the pane between the check and the access since `&mut self` is held). However, using `unwrap()` on fallible lookups in production code is fragile if the pattern is ever refactored. Should use `expect("checked above")` at minimum for diagnostic clarity.

---

## Summary Table

| Severity | Count |
|----------|-------|
| CRITICAL | 3     |
| HIGH     | 8     |
| MEDIUM   | 21    |
| LOW      | 3     |
| **Total**| **35**|

---

## Critical Findings Summary

1. **`handle_pty_death` in `pty_lifecycle.rs` (line 166–167)**: acquires `agent_tracker` (lock 4) before `mux` (lock 1) — violates the mandated lock ordering mux(1) → agent_tracker(4). Any code path that holds `mux` and waits for a background thread that calls `handle_pty_death` could deadlock.

2. **`spawn.rs::spawn_hidden_agent_impl` nested thread** reproduces the same `agent_tracker` → `mux` inversion via `start_backend_reader` → `handle_pty_death`. The violation is in `handle_pty_death` itself and manifests on any backend reader exit path.

3. **Orphaned pane on partial spawn failure** in both `orchestrator.rs::spawn_agent` and `spawn.rs::spawn_visible_agent_impl`: a pane is created and partially mutated before role assignment; if role assignment fails, the pane is leaked in the mux indefinitely with no cleanup path.

<!-- revision: 1 -->
