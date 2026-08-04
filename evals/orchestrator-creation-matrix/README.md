# Orchestrator-creation test matrix (routes B–H)

Task `task_1800fa9977ea418094bb4ab291b2e3f2` (plan `pl_dd93d764e1b94293b5e943af71dfdb5b`).

Every orchestrator-creation route, executable: exact command + observable pass
condition, run against **dev 7686 only** via the `evals/_lib/lib.sh` `dc`
wrapper. A bare `codeterm` here would route to prod 7685 — forbidden.

## Run

```bash
# dev must be healthy on 7686 (orchestrator owns dev lifecycle)
bash evals/orchestrator-creation-matrix/run.sh
```

Exit 0 = no hard FAIL. Exit 1 = at least one FAIL. Exit 7 = dev not up.
The script self-cleans (closes every tab/plan/workspace it created) via an
`EXIT` trap, so scenarios stay independent across reruns.

## Routes

| Route | Command under test | Core observable |
|-------|--------------------|-----------------|
| **B** CLI pair | `orchestration new --orchestrator claude:opus --planner claude:fable --workspace <ws>` | returns plan+2 tab ids; `tab list` shows both; `plan get --json` binds owner **and** planner ref; orchestrator chat carries the peer greeting; **B.IDLE** waits 60s before any goal and asserts the Planner made zero tool calls / file reads |
| **C** role spawn | `agent spawn claude --role Orchestrator --workspace <ws> --task …` | onboarding carries the peer-pairing mandate block with a pre-filled planner spawn command; unpaired nudge fires next cycle |
| **D** promotion | `POST /api/tabs/{id}/make-orchestrator` (no CLI subcommand exists) | role flips to Orchestrator; **session survives**; mandate delivered; unpaired nudge next cycle |
| **E** planner bind | `agent spawn claude --role Planner --plan <id> --parent <orch>` | plan planner ref binds to the new tab; planner onboarding names the orch tab; **nudge stops** |
| **F** waiver | `plan update <plan> --planner none --note "solo test"` | waiver recorded (`planner.kind=waived` + note); nudge stops; `--planner none` **without** `--note` refused |
| **G** doctor ladder | `plan doctor <plan> --kind quality` | `planner_unpaired` = WARNING while unpaired+planning → ERROR on approval attempt → silent once paired/waived |
| **H** negatives | see below | actionable error + **zero object leak** on every failure path |

**H negatives:**
1. `agent spawn --role Orchestrator` with **no** `--workspace` → actionable error, zero tabs.
2. A Worker attempting `orchestration new` / `spawn --role Orchestrator` → refused by the hierarchy guard.
3. `orchestration new` with an unknown provider → clean fail, no half-pair (plan absent, or degraded + recovery command printed).

Route **I** (in-place picker UI: pair confirm transforms the same tab) is the
Playwright picker spec's job (scenario 7 of `task_234c14f04d9f4dbfb5a743d50486617d`),
re-run as a smoke after the eight-lane UI merge. It is intentionally **not** in
this bash matrix.

## Verdict vocabulary

- **PASS** — observable pass condition met on dev.
- **FAIL** — condition not met; a real product bug. Fails the run (exit 1).
- **FINDING** — behaviour diverges from the pinned expectation but is arguably
  by-design or owned by another lane. Tabulated; bounce, not a blocker here.
- **GATED** — the pinned condition rides the async heartbeat nudge
  (`nudge_interval_secs` default **1800s**) and is **not** observable in a
  bounded QA run. The deterministic **driving state** (planner-binding
  resolution: Unbound → nudge fires; Planner/Waived → nudge stops) is asserted
  instead and reported honestly — never silently passed.

### Why the nudge is GATED, not skipped

`planner_pairing_nudge_prefix` (`service/plan/peer_mandate.rs`) is a pure
function of the resolved planner binding: `Unbound`/`OrchestratorFallback` →
nudge; `Planner`/`Waived` → `None`. The prefix is delivered on the plan
reminder heartbeat (`hierarchy/plan_reminder_nudge.rs`) which is timer-gated at
1800s by default. So routes C/D/E assert the **binding state** that drives the
nudge (synchronous, deterministic) and mark the literal heartbeat delivery as
GATED. A live end-to-end nudge assertion would require overriding
`orchestration.nudge_interval_secs` to a few seconds in dev config and polling
`tab chat` — out of scope for a bounded matrix.

## Known findings surfaced while building the matrix (source-read, pre-run)

These are candidate bounces the run will confirm or refute on live dev:

- **C/D mandate uses a literal `PLAN_ID` placeholder**, not a real plan id.
  `orchestrator_onboarding_body` (`service/orchestrator_onboard.rs`) documents
  this: no plan is claimed at role-spawn/promotion time. The pinned C condition
  wants a *real* plan id pre-filled → divergence, bounce to the
  make-orchestrator / pair lane.
- **No `codeterm orchestrator promote` CLI.** Make-Orchestrator is only the REST
  RPC `POST /api/tabs/{id}/make-orchestrator`. Route D uses `dev_post`.
- **No backend hierarchy guard** on `orchestration new` / `spawn --role
  Orchestrator` keyed off caller role. "Worker never spawns" is a prompt-level
  charter, not enforced in the service. Promotion *does* reject tabs that
  already have a parent (`validate_make_orchestrator`). H2 confirms/bounces this.
- **`planner_unpaired` doctor rule** was not located in the quality-doctor
  finding set (`service/plan/gates.rs` emits `paused_task_missing_reason`,
  `stale_open_gate`, …). If absent on live dev, G bounces to the doctor/QA
  enforcement lane `task_084e2148`.

## Wiring into the FINAL acceptance gate

The FINAL acceptance checklist (`task_30b13c9be185472f982b0c26a6182235`) must
list this matrix as a required green gate:

> Orchestrator-creation matrix — `bash evals/orchestrator-creation-matrix/run.sh`
> exits 0 (no FAIL); all FINDING/GATED rows triaged/bounced.
