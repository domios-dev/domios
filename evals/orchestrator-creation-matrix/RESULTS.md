# Orchestrator-creation matrix — run transcript & verdicts

Run context: dev daemon on 7686, builds observed **ea2e30e9 → 905e2d00**
(orchestrator restarted dev several times mid-session; DB write contention and
load spikes to **15–23** on this 10-core weak machine while it built other lanes).

Two classes of result below:
1. **LIVE** — executed against dev this session; verdict is real.
2. **DEFERRED (load)** — the harness is written and syntax-valid, but the
   scenario spawns real `claude` tabs (route B/C/D/E/H1/H2/H3 need 1–8 live
   agents). With 1-min load at 15–23 (> the >10 skip threshold in the worker
   charter), spawning a fleet of agents would risk a 40+ spike. These must be
   run by the orchestrator in a low-load window: `bash evals/orchestrator-creation-matrix/run.sh`.

## Live verdicts (this session, zero/low-agent scenarios)

| Check | Verdict | Evidence |
|-------|---------|----------|
| **F.note_required** | **PASS** | `plan update <p> --planner none` → `--note is required when --planner none (waiver must explain solo intent)`, rc=1 |
| **F.waiver** | **PASS** | `plan update <p> --planner none --note "solo test"` → `OK`; `plan get --json` → `planner={"kind":"waived","note":"solo test"}` |
| **F.waiver_flaky** | **FINDING** | first waiver attempt under peak load returned `✗ plan_index_mirror is pending after a committed write` and read back `planner=null`; retry persisted it. `task_354c755f6e` class regression under load — single-shot `plan update` is not retry-safe. |
| **F.nudge_state** | **PASS** (driving state) | `planner.kind=waived` ⇒ `planner_pairing_nudge_prefix` returns `None` (nudge stops). Heartbeat delivery itself is timer-gated (1800s), GATED. |
| **G.planning / G.approval** | **FINDING** | `plan doctor <p> --kind quality --json` on an unpaired planning plan → findings `missing_goal` (error), `missing_spec_source` (error). **No `planner_unpaired` rule at any state.** The pinned WARNING→ERROR→silent planner ladder is **not wired** in dev build 905e2d00. Bounce to doctor/QA-enforcement lane `task_084e2148`. |
| **G.silent** | n/a | trivially "silent" only because the rule never fires; the ladder is untestable until the rule exists. |

## Structural findings (source-confirmed; confirmed by CLI probes)

| Route | Finding | Bounce target |
|-------|---------|---------------|
| **D** | No `codeterm orchestrator promote` subcommand exists (`orchestrator` has only status/learn/heartbeat/handoff). Make-Orchestrator is **only** `POST /api/tabs/{id}/make-orchestrator`. Route D in the harness uses `dev_post` (dev-locked). | promotion/CLI lane |
| **C / D** | Orchestrator onboarding ships a **literal `--plan PLAN_ID` placeholder**, not a real plan id (`orchestrator_onboarding_body`, by design — no plan is claimed at spawn/promote time). The real orchestrator **tab** id *is* pre-filled (`--parent <tab>`). Pinned C wants a real plan id ⇒ divergence. | make-orchestrator / pair lane `task_aa01c0db` |
| **H2** | No **backend** hierarchy guard on `orchestration new` / `spawn --role Orchestrator` keyed off caller role. "Worker never spawns" is a prompt-level charter only. Promotion *does* reject tabs with a parent (`validate_make_orchestrator`). Harness attributes `orchestration new` to a Worker via `spawned_by_tab_id` and expects refusal; likely a FINDING (not enforced). | hierarchy-guard lane |
| **B/C/D/E** | `plan create --json` returns the id at `.plan_id`; `plan get --json` returns the plan object at top level (`.planner`, `.owner`). `plan claim` has no explicit `--tab` (it claims for the caller tab), so `dc` cannot set ownership; E binds the planner owner-independently. `orchestration new` result = `{plan_id, orchestrator_tab_id, planner_tab_id, warning?}`. | (harness notes) |

## Harness / dogfood friction (bugs in the system we are building)

- **`plan create` intermittently fails `database is locked`** under concurrent
  daemon load — the plan store serializes writes. `mk_plan` retries 5×. Bounce
  to plan-store write-contention.
- **`plan_index_mirror is pending after a committed write`** false-failure
  recurs under load (see F.waiver_flaky). `task_354c755f6e` marked done but the
  race still bites at load 20+.
- **`plan update`/`plan create` return rc=0 even on a `✗ …` error line** — the
  CLI does not set a non-zero exit for these soft failures, so scripts must
  grep stderr rather than trust `$?`.
- **`dc` no-ops under high load.** The lib's `dc` wrapper gates every call on
  `curl --max-time 1 /api/health`; at load 20+ that 1s probe times out and `dc`
  returns exit 7 **without running the command** (safe-by-design: it refuses
  rather than risk leaking to prod). Consequence: run this matrix only when dev
  load is sane — otherwise scenarios silently skip. Not a bug to fix in this
  lane (shared `evals/_lib/lib.sh`), just an operational constraint.

## Not run here (by design)

- Route **I** (in-place picker UI, same-tab transform) → Playwright picker spec
  scenario 7 (`task_234c14f04d9f4dbfb5a743d50486617d`), re-run as post-merge smoke.
