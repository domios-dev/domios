# Decision Gates Lane Notes

## Status: HELD — waiting for task_48f06a32 (planner_ref + prompt constants foundation)

## Exploration findings

### Key files for this feature:
- `src-tauri/src/plan/types.rs` — `Plan`, `Task`, `OrchestratorRef`, `OrchestratorHeartbeat`
- `src-tauri/src/service/plan/mod.rs` — `PlanStore`, `active_owned_plan_frontier_for_pane`
- `src-tauri/src/service/plan/state.rs` — `PlanStateSnapshot`, `build_plan_state_snapshot`
- `src-tauri/src/service/plan/heartbeat.rs` — heartbeat reinjection rendering
- `src-tauri/src/service/plan/lifecycle.rs` — `park_reason` set/clear
- `src-tauri/src/api/plan_routes/mod.rs` — route registration pattern
- `src-tauri/src/api/plan_routes/tasks.rs` — task route pattern with `notify_owner_task_in_review`
- `cli/src/cmd/plan.rs` — CLI plan dispatch pattern (`run_plan` match on `PlanAction`)
- `cli/src/cmd/plan/` — plan subcommand modules (qa_cmd, task_cmd)

### What NOT to implement yet (depends on task_48f06a32):
- `planner_ref` resolver (use `plan.planner_ref` if available, orchestrator fallback)
- Shared prompt constants/snippets for orchestration docs

### Planned implementation scope (post-rebase):
1. `DecisionGate` type in `src-tauri/src/plan/types.rs` (or new `gate.rs`)
2. Gate store CRUD in `src-tauri/src/service/plan/gates.rs`
3. API routes in `src-tauri/src/api/plan_routes/gates.rs`
4. CLI `plan decision ask/answer/list` in `cli/src/cmd/plan/gate_cmd.rs`
5. Heartbeat render extension for open gates (age, blocking count, routed-to pane)
6. Doctor rules: blocked task without gate → error; stale open gate → warning
7. Frontier semantics: gate only parks `blocking_task_ids`; unblocked tasks dispatch normally
8. Escalation: re-ping after TTL, then surface to user
9. Tests: gate lifecycle, routing, frontier never-freeze, CLI parser/command

### NOT doing:
- Frontend header chip (minimal API surface only if backend already has it)
- Global pane guessing (use planner_ref from plan, fallback to orchestrator)
