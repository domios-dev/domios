# Orchestration combat test — findings log (2026-07-07)

Live end-to-end test of the orchestration machinery: a real effort
(retired «Plan views UX redesign» tail) was re-planned and executed by an
Orchestrator (Elgar) + Planner (Sagan) pair with ~11 worker lanes, observed
end-to-end by a toolsmith session. This file is the raw findings ledger;
unfixed items are filed as tasks on plan `pl_n4doq2`.

## Findings

### F1. Role conflict between spawn-setup and incoming brief — no protocol
An external brief addressed the Planner as if it were the Orchestrator; the
agent stalled ~4 minutes deliberating identity. Prompts have no rule for
resolving contradictory role signals.
**Fix direction:** one line in planner/orchestrator suffixes: trust daemon
state (planner_ref, plan ownership) over incoming text; treat briefs as
input material. — *open (prompt change, needs eval run)*

### F2. Dead parent edge on planner Tab
`CODETERM_PARENT_TAB_ID` pointed at a Tab id that no longer exists
(pre-restart id). Hierarchy env vars go stale across daemon restarts.
**Fix direction:** covered by the runtime-relink work (their t11) +
spawn-time parent resolution (F6). — *open*

### F3. `spec section add --parent` rejects handles
`push_subsection` matches `s.id == parent` only; `show`/`locate` accept
handles. Selector inconsistency cost the Planner a debugging detour.
**Fix direction:** resolve handle→id at the service boundary for section
parents (and audit other mutation selectors). — *open (task filed)*

### F4. doctor read plan.json → “0 checked” silently ✅ fixed `2e951ff4`
CLI-side doctor/prune globbed extinct plan.json after the yaml migration;
0-checked read as “clean”. Ported to yaml with task assembly.

### F4b. doctor missed goal.yaml → false `missing_goal` ✅ fixed `765dd92f`
Follow-up to F4: goal lives in its own file; doctor reported a missing goal
on plans whose goal was set, blocking an approval gate mid-test.

### F5. Usage-headroom check skipped under momentum
ORCHESTRATION requires `codeterm usage latest` before heavy spawning; the
orchestrator dispatched 11 opus lanes without it (headroom happened to be
fine). Doc guidance loses to momentum.
**Fix direction:** move the rule into orchestrator_suffix (short line) or a
mechanical pre-spawn hint when N Tabs spawn within M minutes. — *open*

### F6. Worker parentage attached to the wrong orchestrator
Workers spawned by the orchestrator got `parent_tab_id` /
`report_target_tab_id` pointing at ANOTHER orchestrator-role Tab in the
workspace (spawn fallback picked the senior orchestrator, not the caller).
Report ROUTING still reached the true spawner via the hierarchy service —
so the UI tree lied while the loop worked: three disagreeing sources of
parent truth (Tab.parent, Tab.report_target, hierarchy edges).
**Live remediation:** Tabs re-parented via `agent link`.
**Fix direction:** spawn parent defaults to the CALLER Tab; single source
of truth audit. — *open (task filed)*

### F7. ENOSPC silent stall — the worst one
11 lane worktrees × cargo targets filled the disk. Two workers stalled
alive-but-idle; one tried to send its blocked-report and THE REPORT WRITE
ITSELF failed on ENOSPC — the orchestrator stayed blind, the system looked
“running” indefinitely. Distinct failure mode from tab-GONE (their t9
watchdog would not have caught it).
**Fix direction:** (a) `agent report` must fail LOUDLY and retry;
(b) pre-spawn free-disk guard + reclaim finished-lane targets between waves
(orchestrator learning already recorded by Elgar); (c) alive-but-stalled
detection: no report AND no Tab output progress over a window. — *open
(tasks filed)*

### F8. Reminders/nudges gated on `mode == Execution`; nothing flips it
The periodic scheduler reminder channel filters plans to
`PlanMode::Execution`. The orchestrator flow ends at `approve` and nothing
transitions approved → execution, so the whole reminder layer was silently
inert for the entire run (and likely for every effort driven this way).
**Fixed:** `with_plan` now flips Approved → Execution when any task is
in_progress (covers update/batch/qa/respawn paths) + live `plan mode --set
execution` applied to the running effort. — ✅ code landed, ships with next
daemon install

## Confirmed-good behaviours (for balance)
Docs read before acting; stop-on-ambiguity instead of guessing;
evidence-first research (4 parallel Explore agents); no polling while
waiting on reports; acceptance with planner consultation; decisions
recorded as strides; follow-ups become backlog notes, not scope creep;
8/13 tasks done with zero bounces in the first ~2h; lane isolation
(worktrees + lane/* branches) flawless; report format discipline exact.

<!-- revision: 3 -->
