# Team-to-plan binding audit

## User invariant

A plan belongs to exactly one Team. It is not owned by a tab, tab, or workspace.
Every Team member inherits that Team plan. A workspace may only display Teams and
their bound plans; it must not offer a standalone workspace-plan surface.

## Live evidence

`team_818bb2a5` contains the current orchestrator (`166df18d`) and planner
(`02d1339c`). The plan index currently reports two relevant active plans:

- `pl_iassb6`, `Automatic PTY delivery guarantee (recovered from pl_ghk358)`,
  is already bound to `team_818bb2a5`.
- `pl_as7ty5`, `General 1.7.2 plan`, has no `team_id`.

The intended command `codeterm team adopt-plan team_818bb2a5 pl_as7ty5` is
therefore a deterministic one-to-one conflict. It must fail immediately with
`TeamAlreadyHasActivePlan`, naming `pl_iassb6`; it cannot correctly attach both
active plans to the same Team.

## Current model and defects

The model is partly Team-based but not exclusive:

- `src-tauri/src/service/hierarchy/first_plan.rs` creates a Team for an
  orchestrator and creates the initial plan with that Team id. This is the
  correct creation path.
- `src-tauri/src/service/team.rs` authorizes a Team manager then delegates
  adoption to `PlanStore::bind_plan_to_team`.
- `src-tauri/crates/codeterm-plan/src/service_plan/mutations/plan_ops.rs`
  serializes create/bind with `team_plan_lock` and enforces one active plan per
  Team, but its public `create_plan` still passes `None`, so teamless active
  plans are allowed.
- `src/components/Sidebar/SidebarWorkspaceTabs.tsx` deliberately displays
  Team-bound plans in Team cards and filters them out of the standalone row.
  This is correct for bound plans.
- `src/components/Canvas/workspacePlanEntry.ts` and
  `src/components/Sidebar/WorkspacePlanEntry.tsx` still derive and render the
  standalone workspace plan entry from project paths. This violates the user
  invariant and is the button/surface that must be removed.
- `src/components/Canvas/CanvasEmptyActions.tsx` retains a non-Team path named
  `Create plan & orchestrator`. Plan creation should instead require or mint a
  Team and report the resulting binding, never create a standalone plan.

## Why the active CLI commands appear to hang

The current CLI client in `cli/src/client.rs` writes a JSON-RPC request and then
calls `BufRead::read_line` with no response deadline. `team.adopt-plan` and
`plan task add` therefore wait indefinitely if server dispatch is blocked.
The server paths are synchronous and hold plan locks while they persist YAML,
mirror the plan index, and update the in-memory index:

- adopt: `src-tauri/src/socket/server/team.rs` ->
  `ServiceContext::adopt_plan_for_team` -> `PlanStore::bind_plan_to_team`
- task add: `src-tauri/src/socket/server/plan_task_rpc.rs` ->
  `PlanStore::add_task` -> `PlanStore::with_plan`

`with_plan` persists and mirrors while holding the per-plan mutex. This audit
does not claim the exact current blocker without a captured blocked stack, but
it proves the missing failure boundary: a blocked daemon path becomes an
unbounded silent CLI wait rather than an actionable result.

## P0 implementation acceptance

1. Remove standalone workspace-plan entry/navigation and the non-Team empty
   canvas creation path. No active plan can be created without a Team binding.
2. Keep Team cards as the only plan entry. A Team manager and every descendant
   Team member resolve the same Team plan; a tab never stores its own plan
   binding.
3. Make adoption return promptly and deterministically:
   - binding a teamless plan to an empty Team succeeds and emits `plan-changed`;
   - binding a second active plan returns a structured 409/RPC conflict naming
     the existing plan;
   - rebinding an already Team-bound plan to a different Team returns a
     structured conflict;
   - no path waits forever. The client and server must expose a bounded request
     deadline and operation name; a deadline is failure only, never readiness.
4. Make task mutations return a structured bounded failure when plan persistence
   or mirror work cannot complete. Do not hold the per-plan lock across
   unbounded mirror/index work.
5. Provide an explicit Team-plan query/API for the UI rather than inferring plan
   ownership from workspace filesystem paths.

## Deterministic tests

- Rust: create Team A, create its active plan, attempt to bind a second plan;
  assert `TeamAlreadyHasActivePlan` with Team id and existing plan id. Exercise
  the socket RPC and assert one response arrives under a bounded test deadline.
- Rust: concurrent create/bind preserves exactly one active Team plan. Existing
  store coverage covers part of this, but add RPC/client deadline coverage.
- Playwright: create an orchestrator Team through the visible UI; assert the
  Team card exposes the plan by accessible name and its members open that same
  board. Assert no standalone workspace plan button/row exists.
- Playwright: attempt the second-plan/adopt flow and assert the accessible
  conflict alert names the already bound plan, without a polling or sleep wait.
