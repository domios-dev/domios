# Isolated provider prompt matrix

A cell passes only when a real provider's prompt is observed before its policy decision, the permitted answer reaches the provider or exactly one notification reaches its owner, and both the deciding path and duplicate-observation barrier are read from that cell's journal. Permission cells additionally retain the same request, record zero answers, and leave the requested tool effect absent.

This is phase-one source coverage for plan `pl_blazzd`, task `COD-124` (`task_p7qsgp`). No harness, provider, build, project test, or dev gate ran in the Worker lane. The Orchestrator explicitly deferred all executable proof to canonical after Courier, Lookout, Lens and Auditor merge. The initial RESULTS.md is an inventory, not a test result.

## Isolation and scope

Only login/onboarding uses an empty provider HOME, CLAUDE_CONFIG_DIR, CODEX_HOME and XDG directories. All other cells use the owner's existing auth in place, as instructed by Brahms; no credentials are copied. Bad models are rewritten inside the provider wrapper after CodeTerm's catalogue validation. Permission sessions strip full-access flags and use default approval policy. A permission task requests a file outside its workspace; the oracle rejects any creation of that file.

Every daemon has a fresh HOME under `target-check/evals/prompt-matrix/<instance>/daemon-home`. Production 7685 and shared dev 7686 are refused. Provider wrappers are first on this daemon's PATH, use the provider manifest's cliBinary, and record tab identity, PID, executable, cwd and home paths without reading auth contents. Older provider packages install only under the fixture's npm prefix; its npm cache is also isolated. The wrapper prepends that prefix for the selected update provider. Non-tab updater processes retain the isolated daemon HOME.

The runner refuses a noncanonical checkout and a mismatched prepared instance/port. It compares the named daemon's health build_sha to canonical HEAD. It uses evals/_lib/lib.sh for every CLI call through `dc`; `dev_run` injects only the selected connection into the collector. SSE and health are read directly only because the CLI has no event-subscription verb. No production CLI fallback exists.

The live runner takes one nonblocking instance lock. Canonical QA must also remain serialized per physical machine under the Orchestrator's authority. Start and stop are separate exact gates run in a Team-bound utility shell, with build.sh owning the build gateway. A busy gateway or occupied port is a blocker, never a retry.

## Coverage and policy

| Case | Provoker | Required observable result |
|---|---|---|
| trust-hidden | Real mem worker in its host-provisioned directory | Recognized trust frame, authorized responder journal, provider ready; no permission answer |
| trust-cron | Manually triggered real cron in a fresh owner directory | Owner-facing unresolved trust, one routed notice; no automatic trust without provenance |
| trust-visible | Roleless visible agent in a daemon-provisioned workspace | Recognized trust frame, automatic authorized answer, own bound ready session |
| trust-lane | Visible agent in a fresh linked git worktree | Explicit provenance/trust decision; held fresh owner-created worktree notifies once |
| update | Pinned older real package in the fixture npm prefix | Real update prompt, provider-declared safe answer or exactly one owner notice, correct far end |
| login | Empty provider home, no auth env vars | First-run onboarding/login retained or safe onboarding progression; one owner notice at the required unresolved state |
| permission | Ready default-approval session, outside-workspace tool request | Same exact permission request, no answered audit/notification, no tool effect, completed duplicate-observation barrier |
| bad-model | Rewrite provider model argument to an intentionally invalid id | Real provider failure and one owner notice; catalogue rejection alone cannot pass |
| crash | SIGTERM the unique native provider PID under that tab's own launch tree | Provider absence and one owner notice or qualified recovery within the captured restart bound |

A filesystem lane is deliberately created in a temporary repository; it does not dispatch a mutating project Worker. Its lack of host-created provenance must not authorize trust. Providers that do not show a requested prompt do not silently pass. Cursor's manifest binary is checked; absence, quota, unsupported prompts or failed launch must be recorded and explicitly accepted by the owner before any release exemption.

The login provoker is owner-facing; delegated safe-onboarding behavior is a separate observation of the trust-hidden launch. It does not press any key itself. Neither the runner nor its observer agent answers a provider prompt. There are no calls to tab respond or raw key injection.

## Evidence and path attribution

Each invocation creates `transcripts/<provider>-<case>-<jev>-<variant>/<run-id>/` with:

- transcript.jsonl: ordered commands, CLI output, events, tab/lifecycle snapshots, screens, notifications, journal observations, launch records and cleanup;
- terminal.cast plus its screen capture after a successful capture start;
- result.json: the cell status and independent oracle evidence;
- FINDING.md for an executed failure.

The case has a 180-second global failure deadline; the outer process group has a 240-second cap including cleanup. Individual CLI calls cap at 35 seconds, npm at 90, event subscription at 10, token stdin at 10, and teardown at 35. Caps are failures, never readiness or a reason to raise/retry. Event-driven observation waits consume the SSE stream; overflow invalidates evidence. Permission and dedup checks require distinct completed observation ids, not time spent waiting.

Auditor must emit `decided_by: adapter | heuristic | fallback(<reason>)` and `observation_id` per decision point. The runner maps only these explicit values. It does not infer a path from Jev being connected, an action's name, screen wording, or a disappearing prompt. It retains heuristic-arm records, adapter responses, fallback reasons and disagreements separately. A Jev-on cell cannot pass without primary-arm evidence; both arms must be represented.

contracts.json intentionally marks the decision contract pending. After Auditor merges, bind the final tab/point/prompt correlation, arm fields and observation-barrier semantics, then change it to bound. Its point_field and case_points must name the real journal field and case-to-decision mapping; unrelated observation ids cannot provide a duplicate barrier. The exact covered-point list must name id/provider/case. Disagreement cells remain pending until their provokers and the final paired-verdict fields are bound; they must show different heuristic and Jev verdicts for the same observation and identify which one decided the action.

## Pending dependency bindings

These are explicit source gaps, not executed failures:

- Courier: bind the consent trigger, one-run update journal and per-tab version evidence. The default coordinator must update once, safely skip all prompt tabs, and verify the resumed tab environment's version. Busy must prove install completion precedes the busy tab's eventual idle/restart. Fail must prove zero restarts and exactly one notice. scenarios.py currently refuses all coordinator variants.
- Lookout: CLI and outcome journals are bound after ancestry-preserving integration of 49be4862d and b1c93918e. sentinel_scenarios.py authors git landing, fake CI, disconnected regex and exhausted-budget watches through create/list/show/stop. Auditor attribution still gates these live Sentinel cells as PENDING before creation. A separate activity-feed regression bypasses pending attribution and must expose the known feed defect.
- Auditor: bind per-decision journal evidence, the covered-point list and genuine disagreement provokers. Missing fields cannot be bypassed by editing a reported status to PASS.
- Cursor updates need a reviewed archived executable; there is no pinned npm fixture declared. Windows crash needs a supported own-tab process-tree surface. Neither is treated as accepted coverage.

The installed provider CLI may spawn a native child beneath a wrapper. Crash traverses only the recorded launch's descendants and requires one exact provider executable; ambiguous trees are refused. Windows crash is explicitly pending rather than guessing a PID.

## Source facts and deltas

At the lane base, evals/prompt-matrix did not exist. evals/_lib/lib.sh already supports named instances and checks health identity; no shared-lib repair was made.

At the lane base, provider_update.rs requires all provider tabs to be idle before installation, and resumes paused tabs even after a stopped updater fails. Those differ from the requested busy/fail coordinator cells. They were reported to Brahms with source evidence and are Courier's responsibility; the tester made no production fix.

At initial discovery the installed CLI had no sentinel, tab wait, events or journal verbs. The merged source and generated CLI contract now include sentinel create/list/show/stop; new CLI smoke remains deferred. mem worker spawn and cron create support fixed --task fixtures; Brahms explicitly confirmed the pad-only message rule does not prohibit fixed bench test data.

## Bound Sentinel outcomes

Git creates a temporary repository with commit X absent from branch watched, observes the nonlanding verdict, advances that branch to X, and requires exactly one creator notification record. The creator is an owned shell tab in the observer's workspace/Team, exercising the documented shell notification route. Fake CI invokes ci_fixture.py to publish running -> success -> failure atomically; each route must appear once in the ready observer's own bound-session transcript. A route claim cannot substitute for this recipient evidence.

sentinel_probe.py reports each exact fixture read over a nonce-bound localhost socket without changing stdout. Two repeated probe process signals plus the persisted unchanged reason provide the silence barrier. The watcher uses the supported 10-second cadence; the harness uses no sleeps or polling interval. Own-creator model_call journal counts prove unchanged output avoids extra model requests. Listener input, queue, transcript, subprocesses and lifetime are bounded; signal failure fails the cell.

The regex-no-jev variant temporarily selects the empty adapter on the isolated instance, preserving its stored token, and restores the exact prior adapter on cleanup. Its --jev on row means the connected-on phase before the intentional disconnection; an actual fallback path still requires Auditor's journal. Budget-on sets an explicit cap of one, consumes a real request if necessary, and requires budget.reached, a budget_exhausted decision, zero route claims/actions/receipts, and an unchanged-probe barrier. A separate regex watch must still reach the recipient while that cap is exhausted. Prior cap/adapter values are restored exactly, including null as no cap.

Brahms accepted budget-off as N/A on 2026-10-02: disconnected regex never spends model budget; the Jev-on cap plus regex companion covers exhaustion. This is the sole explicit exception in contracts.json and RESULTS.md. No live command is requested for it. Other cells remain subject to full evidence. Current Sentinel journal fields lack decided_by/observation_id and paired arm fields, so contracts.json separates bound CLI/outcome surfaces from pending attribution.

Lens's safe activity projection is time/team_id/kind/label/confidence_band/action/reason/budget_reached/fallback. It currently excludes Sentinel rows, which Brahms confirmed is a defect. The raw own-instance journal remains valid for outcome correlation by sentinel_id/creator, state_hash, hashed bucket label, and decision -> route_claim -> completed action order. budget.calls_per_hour null means uncapped. These are source facts, not executed results.

The activity-feed cell creates a real watch with a stop route, observes its persisted verdict/completed action, then reads /api/supporter/activity and requires a new safely projected Sentinel entry. It makes no policy decision itself; its deciding-path column is not_applicable_visibility. The focused Rust regression supporter_sentinel_verdict_is_visible_in_activity_feed feeds both native journal emission paths through persisted, Team-scoped projection. Both are expected red against the current exclusion until the owner's scheduled fix lands. No executed failure or PASS is claimed.

The brief called named-instance gate execution an AGENTS.md lane exception. The supplied AGENTS.md permits gates only in canonical. Brahms confirmed the two-phase sequence, so all execution is deferred. No unanswered real needed prompt was created during source authoring.

## Canonical gates

GATES.md lists one command per cell and setup/teardown gates for the named instance `promptmatrix` on port 7794. If that port is occupied, choose a new explicit port and issue new exact authorizations; do not silently retry elsewhere. Resolve reviewed older versions before dispatch and replace the version variables in the authorization tokens.

The order is prepare -> start -> smoke -> observer -> Jev-off cells -> connect Jev -> Jev-on cells -> report -> remove observer -> stop. The observer is a roleless real provider agent with a minimal receive-only contract; its own readiness is required. Jev token input comes from stdin in canonical; no credential fixture or token file is committed.

Source-only review commands are git diff and file inspection. Executable gates, including the pure oracle checks and report renderer, have not run. Required red-green proof in canonical:

```bash
python3 evals/prompt-matrix/oracle_tests.py --red-demo
python3 evals/prompt-matrix/oracle_tests.py
```

The first must exit nonzero naming the duplicate-notification oracle. The second must pass all 22 rejection/control checks. The additional --red-sentinel-demo must reject a duplicate transition claim. These prove validators, not provider cells. The focused Rust feed regression must first fail for excluded Sentinel rows, then pass after the scheduled repair. For each actual red, retain its result and FINDING.md, report the oracle to the Orchestrator, and rerun only after the owning task's repair merges; the later transcript must prove the same cell green. Every script must be smoke-run against the real named-instance CLI before phase-two completion. No such demo is claimed here.

report.py rechecks claimed PASS records and exits nonzero if any required cell is missing, pending or failed. Its only N/A is the owner's exact budget-off decision recorded in contracts.json. The inventory has 87 cells: 86 executable cells plus that one N/A. Auditor disagreements remain additional pending coverage.
