# Core journeys (release blocker)

The end-to-end owner journeys that broke on BLACKSTAR on one day while every
unit test stayed green. The suite drives a running DEV through the real API:
CLI calls go through the lib's dev-locked `dc`, HTTP and the event stream use
the connection `dev_run` composes, and the Team dialog leg drives the real DEV
window over CDP. Waits are re-reads on the daemon's `/api/events/stream` (or DOM
mutations in the window) under a failure deadline; elapsed time never counts
as success. It spends real provider tokens.

## Run

Canonical checkout only, in one Team-bound QA utility shell, under the
Orchestrator's serialized QA authority. The run needs a named isolated instance
with its own port, already running the exact build under test, with its window
open because J2 drives the dialog. `run.sh` refuses an unset
`CODETERM_INSTANCE` and ports 7685 and 7686 before it contacts anything.

```bash
CODETERM_INSTANCE=curated CODETERM_API_PORT=7697 bash evals/core-journeys/run.sh all --plan   # dry: journeys, steps, evidence paths
CODETERM_INSTANCE=curated CODETERM_API_PORT=7697 bash evals/core-journeys/run.sh all
node evals/core-journeys/lib/evidence-contract.mjs <run-dir>
```

`all` runs `setup`, a bench-owned DEV restart (`./build.sh kill`, then
`./build.sh dev-prebuilt --wait --wait-build` on the same instance; never port
7685), `verify`, then `cleanup`, and exits nonzero if any check failed. Evidence
is `results.json`, `results.md`, `restart.json` and `screenshots/{setup,verify}.png`
under `target-check/evals/core-journeys/<stamp>/`; the last line prints the wall time.
The budget is about 15 minutes; journeys run in parallel where they do not
share the window. A run over budget is reported with its per-journey times.

Operator restart instead of the bench-owned one:

```bash
bash evals/core-journeys/run.sh setup --out target-check/evals/cj-1
# restart DEV through the dev-run skill, wait for real health
bash evals/core-journeys/run.sh verify --out target-check/evals/cj-1
bash evals/core-journeys/run.sh cleanup --out target-check/evals/cj-1
```

Flags: `--providers claude,codex` replaces the provider set (default: every
installed provider, and Claude, Codex and OpenCode are required; a missing one
fails), `--history-kb N` sizes the runtime history probe (default 320),
`--deadline-secs N` sets the ready and reply deadline (default 240), `--all-tabs`
makes every agent tab on the instance part of the restart oracle, `--keep`
skips cleanup.

## Evidence contract

`lib/evidence-contract.mjs <run-dir>` is the machine check of a run directory.
Exit 0: evidence complete and the run passed. Exit 1: an artefact is missing or
malformed. Exit 3: evidence complete but a check failed. It requires:

- `results.json`: schema 1; every phase (`setup`, `verify`, `cleanup`) with the instance name, an
  integer port other than 7685/7686, build SHA and wall time; every check with a known journey id,
  a `verdict` of `pass`, `fail` or `skip` that agrees with `pass`, a reason unless it passed, a duration
  and a start time; at least one check for SETUP, J1, J2, J3 and J4.
- ids in `results.json` `state`: run id, workspace id, every tab id, a session id for each J1 tab
  that answered, and the pre-restart listener, build and tab ids.
- `results.md`: byte-equal to the rendering of `results.json`.
- `restart.json` (`upgrade.json` for the first upgrade cycle): `pass`, phase `verified`, listener
  pid and start time that prove a restart, same instance and port as `results.json`. An operator
  restart (`setup`/`verify` run by hand) leaves no `restart.json` and fails this check on purpose.
- `screenshots/setup.png` and `screenshots/verify.png`, taken with `scripts/dev-screenshot.mjs`
  and recorded in `state.screenshots`; each must exist and be a PNG.

`lib/fixture-run.mjs <dir>` writes a complete synthetic run directory for trying the check.

## Journeys

| Id | Observable pass |
|---|---|
| J1 | Per installed provider: spawn reaches `agent_ready`; an exact marker reply arrives as a new assistant item in the tab's own `/chat` with the same bound session and idle status; a long turn reaches `working`, `tab stop` returns `stopped:true` and the tab returns to idle on the same session. Providers that run without a PTY (`agent_runtime`) also answer after a ~320 KiB filler turn. |
| J2 | `POST /api/orchestration/new` with exactly the dialog's body and no caller claim creates a Team whose Orchestrator and Planner reach ready. Then, in the DEV window, with a Manager tab and then a chat_backend Worker tab selected (`tab focus`, proven by its visible pane), the Start-a-session dialog in Team mode (PTY provider, then runtime provider) closes without an alert and a new two-member Team reaches ready. |
| J3 | A shell-tab Orchestrator fixture (bound session, `set-orchestrator`) creates a Plan and a read-only task and dispatches it to a runtime provider. The Worker's backing is `chat_backend`; the attempt is acknowledged; the task reaches `in_review` with a report; `review bounce` is delivered and answered with a newer report. After the restart: bounce again, newer report, `review accept` reaches `done`. |
| J4 | The listener PID and kernel start time changed. Every agent tab recorded before the restart reaches ready with the same session, backing and `task_binding.attempt_id`; the restored Worker's attempt id is the task's current attempt; J1 tabs and the J2 Manager answer a new exact marker on their restored session. |
| J5 | `upgrade`: build N-1 runs `setup` on a disposable instance, the instance restarts onto build N (`--expect-build-change`), `verify` and `cleanup` run, then the full `all` cycle runs on N. |

A deterministic shell-tab Orchestrator owns J3's review verdicts because a live
Orchestrator would race the bench on the Worker's reports.

## Today's BLACKSTAR regressions

Source facts are what the fix commits changed; "expected failure" is the check
that should fail on the pre-fix build. The pre-fix runs themselves are canonical
QA evidence and are not claimed here.

| Regression (fix) | Expected failing check on the pre-fix build |
|---|---|
| Desk leaf overflow from duplicated Manager prompts (`6f13752e8`, `7a2f073ca`) | J4 `restored_tab_ready` for the J2 Orchestrator/Planner: launch or runtime failure naming the desk leaf bound. |
| Codex history over 256 KiB of plugin state (`8f91597ec`) | J1 `long_history_turn` or J4 `restored_tab_ready` / `restored_send_reply` for the Codex tab: `runtime degraded: SizeLimit`. Whether one live turn's history replay crosses the old bound is a hypothesis; the synthetic over-1 MiB case belongs to `evals/upgrade-restore`. |
| Restored task-bound Workers without `attempt_id` (`e299a8dc2`) | J4 `restored_worker_attempt_binding`: "restored Worker has no attempt_id". |
| `LivePtyRequired` for chat_backend Workers (`restore_binding.rs` requires a PTY) | J4 `restored_worker_attempt_binding` or J3 `restored_bounce_then_report`: the restored runtime Worker cannot re-report. |
| Start-session Team dialog refused with an agent tab selected (task_5s4ue7) | J2 `ui_manager_context` / `ui_worker_context`: the dialog's alert text is the failure reason. |

## Upgrade (J5)

Use a disposable named instance and its own port; every mode refuses 7685, 7686
and an unnamed instance. The N-1 checkout is any worktree at the previous
release (for example `git worktree add /tmp/cj-prev v1.14.8`).

```bash
CODETERM_INSTANCE=cj-upgrade CODETERM_API_PORT=7711 \
  bash evals/core-journeys/run.sh upgrade --from-checkout /tmp/cj-prev --all-tabs
```

It builds and starts N-1 in that checkout, then follows the cycle above with
the stop in the N-1 checkout and `./build.sh dev --wait --wait-build` here. The
N-1 phase drives build N-1's own CLI, so a flag that build lacks fails its
check by name. Snapshot replay of a real desk and synthetic worst cases is
`evals/upgrade-restore` (task_4o2fnb); run both for an app release.

## Offline tests

```bash
node --test evals/core-journeys/*.test.mjs evals/core-journeys/lib/*.test.mjs
```

They cover the verdicts (readiness, exact reply, task and attempt states), the
event-wait loop, the dialog page scripts against a fake DOM, results and
argument parsing. They are not live proof.

## Troubleshooting

| Symptom | Cause | Recovery |
|---|---|---|
| `DEV exposes no cdp_port` or `no page target` | The DEV window is closed or has no CDP endpoint | Open the DEV window (dev-run skill) and rerun; J2's UI legs cannot pass without it. |
| `deadline: Start a session dialog opened` | First-run onboarding is active, so the picker ignores the open event | Finish first run on that instance, then rerun. |
| `restart_not_observed` | `build.sh dev-prebuilt` refused stale provenance or the same listener survived | Run `./build.sh dev --wait` once to record provenance, then rerun `all`. |

<!-- revision: 1 -->
