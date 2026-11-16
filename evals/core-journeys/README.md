# Core journeys (change-scoped release evidence)

The end-to-end owner journeys that broke on BLACKSTAR on one day while every
unit test stayed green. A release runs only the journeys whose source areas its
diff touches; the gates skill's Release section owns that procedure. The suite drives a running DEV through the real API:
CLI calls go through the lib's dev-locked `dc`, HTTP and the event stream use
the connection `dev_run` composes, and the Team dialog leg drives the real DEV
window over CDP. Waits are re-reads on the daemon's `/api/events/stream` (or DOM
mutations in the window) under a failure deadline; elapsed time never counts
as success. It spends real provider tokens.

## Run

Run in one Team-bound QA utility shell under the Orchestrator's serialized QA
authority. The run needs a named isolated instance
with its own port, already running the exact build under test, with its window
open with a CDP port because J2 drives the dialog. Launch it from the canonical
Team DEV utility shell using the dev-run skill, with explicit isolated ports:

```bash
CODETERM_DEV_INSTANCE=curated CODETERM_DEV_API_PORT=7697 \
CODETERM_DEV_VITE_PORT=14997 CODETERM_DEV_CDP_PORT=9397 ./build.sh dev --wait --wait-build
```

`run.sh` fails before any journey or restart if `/api/health` has no valid
`instance.cdp_port`. A window without native CDP cannot run these UI journeys.
It refuses an unset
`CODETERM_INSTANCE` and ports 7685 and 7686 before it contacts anything.

```bash
CODETERM_INSTANCE=curated CODETERM_API_PORT=7697 bash evals/core-journeys/run.sh all --plan   # dry: journeys, steps, evidence paths
# Resolve these values from the named instance catalogue before this paid run.
CODETERM_INSTANCE=curated CODETERM_API_PORT=7697 bash evals/core-journeys/run.sh all --out OWNED_OUT \
  --model claude=EXACT_ADVERTISED_HAIKU_ID \
  --model opencode=deepseek/deepseek-flash \
  --model codex=EXACT_ADVERTISED_CODEX_ID \
  --model acp=gemini-cli
node evals/core-journeys/lib/evidence-contract.mjs <run-dir>
```

For lane-owned bench repairs against an existing candidate, set
`CORE_JOURNEY_BUILD_CHECKOUT` to the candidate checkout's absolute path. CLI
selection and restart use that checkout's validated artifacts and provenance;
the journeys and control runner use the lane's sources. Keep the candidate
checkout and build stable through the run. This does not compile a new product.

With the full suite or any selection that includes J4,
`all` runs `setup`, a bench-owned DEV restart (`./build.sh kill`, then
`./build.sh dev-prebuilt --wait --wait-build` on the same instance; never port
7685), retaining its original frontend and CDP ports, `verify`, then `cleanup`,
and exits nonzero if any check failed. Evidence
is `results.json`, `results.md`, `restart.json` and `screenshots/{setup,verify}.png`
under `target-check/evals/core-journeys/<stamp>/`; the last line prints the wall time.
The budget is about 15 minutes; journeys run in parallel where they do not
share the window. A run over budget is reported with its per-journey times.

On a fresh instance, setup chooses full agent access through the normal
`PATCH /api/config` key `agent.full_access` and reads the choice back before
any spawn. An existing explicit choice is preserved. This is the same persisted
setting as Settings > Agents > Full access for agents; the bench needs unattended
provider execution on its disposable instance.

Operator restart instead of the bench-owned one:

```bash
bash evals/core-journeys/run.sh setup --out target-check/evals/cj-1 "${MODELS[@]}"
# restart DEV through the dev-run skill, wait for real health
bash evals/core-journeys/run.sh verify --out target-check/evals/cj-1 "${MODELS[@]}"
bash evals/core-journeys/run.sh cleanup --out target-check/evals/cj-1
```

Repeat `--model PROVIDER=ADVERTISED_ID` for **every installed provider**, including
providers beyond the three required harnesses. Profile-backed providers use
`--model acp=<profile>`, for example `--model acp=gemini-cli`. The bench validates
the selection against `GET /api/agents/providers/acp/profiles` before configuration,
every launch and later send. The profile must be declared, installed or detected,
without a pending/failed setup or reported authentication requirement. An unknown
auth state does not prove authentication; the real journey must still succeed.
A missing selection refuses with the expected form and ready profile IDs;
it never drops ACP from the roster.

`--model acp=<profile>/<model>` selects an exact value from that profile's declared
`configOptions` with category `model`. Model IDs may themselves contain slashes.
`--model acp=<profile>` keeps the agent's default and passes no model override;
`<profile>/default` also denotes that default when no model options are declared.
The bench clears any instance-scoped model override for a profile-default launch.
Every profile spawn and task dispatch passes `--profile`; an explicit model
also passes `--model`. No vault key is read or copied by the bench: `gemini-cli`
uses its existing `authEnv` injection. Uppercase IDs above are live
preflight prerequisites, never literal IDs. The owner selects Claude Haiku only,
OpenCode exactly `deepseek/deepseek-flash`, and an explicit normal Codex model.
The selector is generic: exact provider-owned IDs are opaque, aliases and default
substitution are refused. Discover with the bench-scoped `dc agent providers
--json` and `dc agent models PROVIDER --json` after sourcing `evals/_lib/lib.sh`.
Never invoke a raw CLI with inherited connection settings. For the operator
commands, `MODELS` is a Bash array containing the same repeated `--model` pairs.

For providers with flat model catalogues, before first-run completion or any launch,
the bench refreshes the catalogue,
validates capability availability, persists instance-scoped provider defaults
and the separate General model, and reads them back. A conflicting General
override, missing selection, missing/unknown catalogue or unavailable model
refuses before paid work. Static catalogues without live discovery are validated
against their current advertised exact entries; a failed required live refresh
never falls back to a cached entry. General, J1 spawn, both API/UI J2 pair roles
and J3 dispatch receive explicit models. Profile-backed launches instead record
the profile catalogue snapshot, exact launch profile and optional model separately.
The required native harnesses remain the API/UI Team pair representatives, whose
creation APIs do not support launch profiles. ACP still runs the complete J1 and
restart journey. The UI checks both rendered selections
before confirming. No global provider profile is changed.

Prelaunch evidence proves what was requested and passed to the provider. It
cannot attest third-party execution. Before later sends or review bounces, exact
requested, catalogue-resolved and actual own-session model identities must be
known and equal. Only J1's initial input may precede runtime identity evidence;
unknown or mismatched identity then refuses every later paid operation. A
policy refusal is persisted and `all` exits 8 without restarting. An ordinary
launch or send failure stays with that journey and does not become a global
model-policy refusal; other correctly configured journeys continue. Verify requires
the same explicit mapping, complete roster and persisted same-session identities;
it refreshes catalogues again before restored sends. Readiness means usable, and
a restored tab's own session reports its model after that, so a restored identity
whose other fields already agree waits, event-driven and for at most 30 s, for the
actual model before any paid turn. A value that is merely not yet known persists no
refusal; any disagreement refuses at once, and a model still unknown at the bound
refuses as unconfirmable. Profile-default sessions
validate their exact launch profile and own session; actual model identity is
recorded only when reported, without claiming an explicit-model attestation.
An explicitly selected profile model requires the same requested/resolved/actual
evidence as a flat model. Historical partial Haiku
or DeepSeek probes are not full-suite evidence.

Flags: `--providers` may only repeat the complete installed roster; narrowing
fails. Claude, Codex and OpenCode remain required unless excluded explicitly
(see *Journey selection*). Other flags:
`--history-kb N` sizes the runtime history probe (default 320),
`--deadline-secs N` sets the ready and reply deadline (default 240), `--all-tabs`
makes every agent tab on the instance part of the restart oracle, `--keep`
skips cleanup.

## Journey selection

`areas.json` is the journey → source-area map, owned by this bench. Each
journey lists repository path prefixes and the journeys it `requires` (J2's
Worker-context leg uses J3's Worker, so J2 requires J3). Edit the map, not the
skills, when an area moves or a journey gains coverage.

```bash
node evals/core-journeys/select.mjs BASE HEAD --out OWNED_OUT/selection.json
```

The selector reads `git diff --name-only --no-renames BASE..HEAD` and, for each
changed path, selects every journey with a matching prefix, recording
`path (area prefix)` as the reason; `required by Jn` records the closure.
Unmapped paths select nothing and are listed in `unmatched_paths`: there is no
run-everything fallback. Unselected journeys are `not_run` with reason
`area untouched`. It refuses a map whose prefixes match no file at HEAD, so a
rename cannot silently stop selection. An empty selection is the not-run record
itself; `run.sh` refuses to start with nothing to run.

`run.sh` selection flags (default: the full suite J1–J4):

- `--selection FILE` runs what the selector chose. The output is copied into
  `results.json`, and the contract recomputes it from its own `changed_paths`.
- `--journeys J1,J4` runs exactly these journeys plus their `requires`. With
  `--selection`, adding a journey is recorded as `added by --journeys`; dropping a
  selected one needs `--waiver` and records it as `waived` (accepted risk).
  Without `--selection`, unlisted journeys are `not in --journeys`.
- `--exclude-provider ID` (repeatable) removes an installed or required
  provider from the roster. It needs `--waiver`; `--providers` still cannot
  narrow and an omitted `--model` still refuses.
- `--waiver "OWNER REASON"` is the owner's accepted-risk statement. It is
  required whenever a selected journey is dropped or a provider is excluded,
  and refused otherwise.

The flags are recorded verbatim in `results.json` (`selection.flags`) and in a
`results.md` text block, with each journey's status and reasons. Not-run and
waived journeys are never passes and must have no checks. Repeat the same
selection flags for an operator `verify`; a different selection refuses.

Without J4, `all` runs `setup` then `cleanup`: no restart, no `verify`, no
`restart.json` or verify screenshot, and J3 accepts its review in setup. With J4
but not J1, setup first spawns one restore fixture per roster provider
(`restore_fixture_ready`, `restore_fixture_reply`) so J4 has restorable sessions
of its own. Its task-bound Worker leg (`restored_worker_attempt_binding` and the
fixture rebind) runs only when J3 is selected.

## Evidence contract

`lib/evidence-contract.mjs <run-dir>` is the machine check of a run directory.
Exit 0: evidence complete and the run passed. Exit 1: an artefact is missing or
malformed. Exit 3: evidence complete but a check failed. It requires:

- `results.json`: schema 1; a `selection` that its recorded flags and selector output reproduce
  exactly; every phase (`setup`, `cleanup`, and `verify` exactly when J4 runs) with the instance name, an
  integer port other than 7685/7686, build SHA and wall time; every check with a known journey id,
  a `verdict` of `pass`, `fail` or `skip` that agrees with `pass`, a reason unless it passed, a duration
  and a start time; at least one check for SETUP and each journey that runs, none for a not-run or
  waived journey.
- Required providers are in the roster unless excluded under the recorded waiver; an excluded
  provider is absent. Launch evidence is required per running journey (`J4-spawn` for J4's own setup).
- ids in `results.json` `state`: run id, workspace id, every tab id, a session id for each J1 tab
  that answered, and the pre-restart listener, build and tab ids.
- `state.model_policy`: exact complete-roster selections, catalogue refresh/capability evidence,
  scoped General/provider configuration, explicit launch records and requested/resolved/actual
  own-session identities before and after restart. Missing policy evidence invalidates historical runs.
- `results.md`: byte-equal to the rendering of `results.json`.
- `restart.json` (only when J4 runs): `pass`, phase `verified`, listener
  pid and start time that prove a restart, same instance and port as `results.json`. An operator
  restart (`setup`/`verify` run by hand) leaves no `restart.json` and fails this check on purpose.
- `screenshots/setup.png` and, when J4 runs, `screenshots/verify.png`, taken with `scripts/dev-screenshot.mjs`
  and recorded in `state.screenshots`; each must exist and be a PNG.

`lib/fixture-run.mjs <dir>` writes a complete synthetic run directory for trying the check.

## Journeys

| Id | Observable pass |
|---|---|
| J1 | Per installed provider: spawn reaches `agent_ready`; an exact marker reply arrives as a new assistant item in the tab's own `/chat` with the same bound session and idle status; a long turn reaches `working`, `tab stop` returns `stopped:true` and the tab returns to idle on the same session. Providers that run without a PTY (`agent_runtime`) also answer after a ~320 KiB filler turn. |
| J2 | `POST /api/orchestration/new` with exactly the dialog's body and no caller claim creates a Team whose Orchestrator and Planner reach ready. Then, in the DEV window, with a Manager tab and then a chat_backend Worker tab selected through its visible sidebar control and proven by its mounted pane, the Start-a-session dialog in Team mode (PTY provider, then runtime provider) closes without an alert and a new two-member Team reaches ready. |
| J3 | A deterministic control provider, launched through the normal provider lifecycle, creates a Plan and a read-only task using its own scoped launch credential and dispatches to the selected real runtime provider. The Worker's backing is `chat_backend`; the attempt is acknowledged; the task reaches `in_review` with a report; `review bounce` is delivered and answered with a newer report. After restart the control provider resumes, reacquires its scoped launch environment, rebinds the Plan, bounces again and accepts a newer report. |
| J4 | The listener PID and kernel start time changed. Every agent tab recorded before the restart reaches ready with the same session, backing and `task_binding.attempt_id`; the restored Worker's attempt id is the task's current attempt; J1 tabs and the J2 Manager answer a new exact marker on their restored session. |

A run-specific control provider owns J3's review verdicts so model-driven
Orchestrator decisions cannot race the bench. Its ordinary launch/resume path
supplies a scoped credential kept only in the runner process environment. Its
SessionStart hook and readback prove its own provider, session and launch nonce.
Fixed CLI requests enter a private AgentPad queue for that nonce; claiming a
request precedes execution, and uncertainty never resubmits it. A resumed runner
uses a new queue generation and never executes the previous launch's requests.
The fixture creates a linked spec, goal and task quality fields, records its
single-task Planner waiver, then passes the normal Plan approval gate before
dispatch. The same scoped Orchestrator performs those actions.
The fixture is recorded separately from the complete paid-provider roster; every
real installed provider still requires an explicit model selection. Cleanup
removes this run's provider after closing its workspace. No fixture credential
is written to results, state, discovery files or request receipts.

Before the restart snapshot, every J2 pair member answers an exact marker on its
own session. This establishes a real conversation for providers that create
resumable history only after their first turn. The restore check still requires
the exact same session, backing and attempt identity. J4 selects each owned tab
serially through visible sidebar controls in the native window to materialize lazy restored tabs before
requiring readiness; selecting a tab alone never counts as ready.
For owned PTY tabs, J4 invokes the supported browser Terminal spawn route to
materialize the saved session; mounting a Chat transcript alone need not start it.
Reply probes use compact nonces derived from the run, provider or tab, and phase;
only an exact new assistant reply in the bound session passes.

For an extra non-gating observation, set `CORE_JOURNEY_OBSERVE_CHAT_ACTIVATION=1`.
Before J4 activation, one still-lazy owned Claude PTY receives one normal Chat
send without pre-spawning it. `chat-activation-observation.json` records the
lifecycle, receipt and exact new own-session user/reply counts. Its outcome does
not enter the gate verdict; uncertain input is never resubmitted.

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

## Offline tests

```bash
node --test evals/core-journeys/*.test.mjs evals/core-journeys/lib/*.test.mjs
```

They cover the verdicts (readiness, exact reply, task and attempt states), the
event-wait loop, the dialog page scripts against a fake DOM, results, journey
selection and argument parsing. They are not live proof.

## Troubleshooting

| Symptom | Cause | Recovery |
|---|---|---|
| `FAIL prerequisite` naming `cdp_port`, or `no page target` | The DEV window is closed or has no CDP endpoint | Launch the named DEV with `CODETERM_DEV_CDP_PORT` as above (dev-run skill); confirm health publishes it and keep the window open. |
| `deadline: Start a session dialog opened` | First-run onboarding is active, so the picker ignores the open event | Finish first run on that instance, then rerun. |
| `restart_not_observed` | `build.sh dev-prebuilt` refused stale provenance or the same listener survived | Run `./build.sh dev --wait` once to record provenance, then rerun `all`. |

<!-- revision: 9 -->
