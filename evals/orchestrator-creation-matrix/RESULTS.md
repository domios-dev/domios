# Orchestrator-creation matrix — run transcript & verdicts

Green run: `bash evals/orchestrator-creation-matrix/run.sh` via `evals/_lib/lib.sh` against the named dev instance meshqa (7687), build **ffef74bbe**, 2026-09-27 (Tester, change note 20). Result: PASS=28 FAIL=0 FINDING=0 GATED=2, exit 0; scratch workspace closed, every matrix-created Tab absent, final Plan set equal to baseline.

GATED rows ride the heartbeat nudge (`nudge_interval_secs` default 1800 s) and are not observable in a bounded run; the deterministic driving state is asserted instead (see E/F).

```
 ORCHESTRATOR-CREATION MATRIX — verdict table (dev build ffef74bbe)
======================================================================
 SCENARIO         VERDICT   DETAIL
----------------------------------------------------------------------
 role-preset      PASS      every creation-route kind resolves through the shared resolver
 B                PASS      returned orchestrator=ecad16ca and planner=0f13c9bc; response has no plan_id and pair crea
 B.tabs           PASS      tab list shows both orchestrator + planner tabs
 B.greeting       PASS      paired Orchestrator launch recorded a non-empty system prompt
 B.idle           PASS      fresh Planner stayed idle for 60s and no Plan existed before the first goal
 B.first_goal     PASS      first plan creation returned pl_vaxcsh after pair response had no plan_id
 B.refs           PASS      first Plan owner=ecad16ca and planner ref (kind=tab tab=0f13c9bc) bind to the pair tabs
 C                PASS      orchestrator tab spawned: fb9b6ae9
 C.mandate        PASS      launch diagnostics confirm a system prompt was supplied
 C.prefill_parent PASS      the launch prompt was present; exact --parent-tab composition is covered by prompt tests
 C.prefill_plan   PASS      the launch prompt was present; exact plan-placeholder composition is covered by prompt tes
 C.nudge          GATED     unpaired nudge fires on heartbeat (nudge_interval_secs default 1800s), not observable in b
 D.role           PASS      tab promoted with role=Manager and role_profile=orchestrator
 D.session        PASS      tab session survived promotion (same id before/after: 'b18d01bc-10d1-4354-890a-15c24056039
 D.mandate        PASS      promotion launch recorded a non-empty system prompt
 D.nudge          GATED     unpaired nudge is heartbeat-gated (1800s); driving state Unbound => should fire
 E.bind           PASS      plan planner ref bound to the new planner tab (kind=tab tab=93dcbfaf)
 E.intro          PASS      Planner launch recorded a non-empty system prompt
 E.nudge_state    PASS      driving state now Planner-bound => planner_pairing_nudge_prefix returns None (nudge stops)
 F.note_required  PASS      '--planner none' without --note is refused (rc=1)
 F.waiver         PASS      waiver recorded (planner.kind=waived note='solo test')
 F.nudge_state    PASS      driving state Waived => nudge prefix None (nudge stops); heartbeat is timer-gated
 G.planning       PASS      unpaired plan in planning mode => planner_unpaired WARNING
 G.approval       PASS      approval attempt renders planner_unpaired as an ERROR
 G.silent         PASS      after waiver => planner_unpaired is silent
 H1               PASS      spawn --role Orchestrator w/o --workspace failed cleanly, zero tabs created
 H2               PASS      Worker-attributed team pair without --acknowledge-leaf was refused with no Plan or Tab cre
 H3               PASS      unknown-provider pair refused before creating a Plan or Tab
 tab_cleanup      PASS      scratch workspace closed; every matrix-created Tab is absent
 plan_hygiene     PASS      every scenario removed its own Plan and the final Plan set matches baseline
----------------------------------------------------------------------
 PASS=28  FAIL=0  FINDING=0  GATED=2
======================================================================
RESULT: no hard failures (triage FINDING/GATED rows)
```

Earlier run on the same build (bench/run.out, executed while a parallel delivery matrix loaded the daemon) failed only B.tabs: the first `/api/tabs` presence check missed the fresh pair tabs, the re-check a moment later found both (orch=True planner=True). Not reproduced in isolation.
