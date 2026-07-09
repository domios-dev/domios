---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_b9e2af3f55594203acd4c3c40ebc8000
handle: branch-review-r2-rust-daemon-core-locks-async-discipline-api-diff-vs-dev
title: 'Branch review R2: Rust daemon core — locks, async discipline, API (diff vs develop)'
status: done
---

Review git diff origin/develop...HEAD (merge-base 40ba275ffc43) RESTRICTED to src-tauri/src/socket/server/, src-tauri/src/api/, src-tauri/src/service/ (EXCLUDING service/plan — R1 owns it), src-tauri/src/agent/, PTY/spawn paths. HARD RULES to audit against: lock order mux>output_buf>screen_state>agent_tracker>pty; parking_lot non-reentrant; NO guard across .await (clippy::await_holding_lock denied); blocking/IO/subprocess in handlers must use spawn_blocking; daemon never shells out to its own CLI; hot-path commands cheap+bounded; capability-branching never provider-id literals; detection keys on the pane's OWN session id. CONCRETE LEAD: the diff adds 29 new .unwrap() calls under src-tauri/src/api + src-tauri/src/socket/server — triage every one (grep '^+.*unwrap()' in the diff), classify panic-reachable-from-request vs infallible, file fixes for the reachable ones. Also check the PTY reader-race fix (prepare_backend_reader) is not regressed. OUTPUT CONTRACT: same as R1 — strides with severity/file:line/fix + detailed fix tasks for cheap models.
