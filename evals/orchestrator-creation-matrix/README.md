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
| **B** CLI pair | `team pair --orchestrator claude:opus --planner claude:fable --workspace <ws>` | returns two tab ids with no `plan_id`; no Plan exists before a first goal; `tab list` shows both; orchestrator chat carries the peer greeting; **B.IDLE** verifies the Planner remains idle and no Plan appears before the first goal; a Plan created as the Orchestrator binds owner **and** paired Planner ref |
| **C** role spawn | `agent spawn claude --role Orchestrator --workspace <ws> --task …` | onboarding carries the peer-pairing mandate block with a pre-filled planner spawn command; unpaired nudge fires next cycle |
| **D** promotion | `POST /api/tabs/{id}/make-orchestrator` (no CLI subcommand exists) | role flips to Orchestrator; **session survives**; mandate delivered; unpaired nudge next cycle |
| **E** planner bind | `agent spawn claude --role Planner --plan <id> --parent <orch>` | plan planner ref binds to the new tab; planner onboarding names the orch tab; **nudge stops** |
| **F** waiver | `plan update <plan> --planner none --note "solo test"` | waiver recorded (`planner.kind=waived` + note); nudge stops; `--planner none` **without** `--note` refused |
| **G** doctor ladder | `plan doctor <plan> --kind quality` | `planner_unpaired` = WARNING while unpaired+planning; `plan approve` renders it as ERROR; a Planner waiver makes it silent |
| **H** negatives | see below | actionable CLI error + **zero object leak** on every failure path |

**H negatives:**
1. `agent spawn --role Orchestrator` with **no** `--workspace` → actionable error, zero tabs.
2. A Worker-attributed `team pair` without `--acknowledge-leaf` → refused by the hierarchy guard, with no Plan or Tab created.
3. `team pair` with valid `claude` Orchestrator and unknown Planner provider →
   clean fail before any Tab or Plan is created.

The matrix invokes the CLI only through `dc` from `evals/_lib/lib.sh`. Plan
creation and mutations carry the real Orchestrator via `dc --as-tab`. The
`chat-raw/start` HTTP helper sends a fresh `subscriptionId` and always stops
that subscription after reading the snapshot. Every failed row prints its
captured CLI or JSON body.

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

## Source findings addressed by the current refresh (live run pending)

- **B pair output has no `plan_id` until the Orchestrator creates its first
  goal/Plan.** The matrix now checks the unbound response and first-Plan binding.
- **C/D onboarding uses the literal `PLAN_ID` placeholder** because no Plan is
  claimed at role-spawn/promotion time. The C prefill oracle accepts that
  contract.
- **No `codeterm orchestrator promote` CLI.** Make-Orchestrator is only the REST
  RPC `POST /api/tabs/{id}/make-orchestrator`. Route D uses `dev_post`.
- **`team pair` refuses Worker and Manager callers unless they pass
  `--acknowledge-leaf`.** H2 exercises the real CLI with a Worker caller.
- **Both providers are checked before pair tabs are reserved.** H3 asserts an
  unknown provider leaves no Tab or Plan.
- **`planner_unpaired` is present in the quality-doctor finding set.** G checks
  its warning, approval-time error rendering, and silence after waiver; the
  older “absent” result came from a harness parser that fed Python source on
  stdin and from expecting approval's transient escalation in a later doctor
  call.

## Wiring into the FINAL acceptance gate

The FINAL acceptance checklist (`task_30b13c9be185472f982b0c26a6182235`) must
list this matrix as a required green gate:

> Orchestrator-creation matrix — `bash evals/orchestrator-creation-matrix/run.sh`
> exits 0 (no FAIL); all FINDING/GATED rows triaged/bounced.

The refreshed harness and expectations have not been executed in this Worker
lane. The Orchestrator must smoke-run the real CLI harness and regenerate
`RESULTS.md` only after a green run.
