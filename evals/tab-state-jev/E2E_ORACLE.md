# Tab-state e2e oracle transcript

## Latest rerun — 2026-10-02, build `fa5e163c7`

### Verdict

**Fail.** Only healthy OpenCode produced the required `spawn-ok`; healthy Claude and Codex were discarded as stale. The proxy and unsupported-model screens were visible, but neither produced its tab-state classification. The login case produced one strong auth classification with no tab-state action; no second agreeing observation arrived. The trust case produced no tab-state classification. No weak hint or tab-state action was observed, so the weak-hint branch and two-observation escalation were not fully exercised. Keep both shipping defaults false.

### Run identity and setup

- Canonical checkout remained on `release/1.12.4` and was not edited, committed, or switched by this run. The named instance health endpoint reported `tabstate`, API port `7697`, version `1.12.3`, build `fa5e163c7d9072469ef9c646804c8c4ea748bb39`.
- The instance was restarted after canonical advanced from the earlier `e3b680069` build. All cases below ran against `fa5e163c7`.
- Temporary Team: `team_cb9a4522`; Plan: `pl_122kvd`; Orchestrator: `4a116298`; Planner: `bce56ddb`.
- The Jev token was piped from `~/.codeterm/supporter/typesafe-jev.json`; its value was never printed. The historical tab-state observation and signal switches were enabled only in this instance during the run, then disabled and reset to false. No shipping default changed.
- Claude and Codex failure wrappers were temporarily disabled for the healthy launches, then restored for their targeted cases. The first Claude attempt accidentally used the proxy wrapper; it was cancelled and excluded. The clean healthy Claude rerun still had no `spawn-ok`.
- Every `dc` command ran under Bash after exporting the named instance variables and sourcing `evals/_lib/lib.sh`. No project tests or other validation gates ran, and no source files were changed.
- `healthy_cursor` was excluded because its earlier launch exited before readiness with no retained diagnostics. `healthy_lmstudio` was excluded because the plugin lacks `chatBackend.sessionConfig` and spawn returns `chat_backend_session_config_required`.

### Per-case results

| Case | Result | Evidence |
|---|---|---|
| Healthy Claude | **Fail** | Clean rerun tab `b57c3888` reached `agent_ready` and bound a session, then logged `stale observation discarded` at `00:16:33Z`. No `launch_ready` tab-state event or `spawn-ok` arrived. |
| Healthy Codex | **Fail** | Tab `4cd3f743` reached `agent_ready` and bound a session, then logged `stale observation discarded` at `00:16:47Z`. No `launch_ready` tab-state event or `spawn-ok` arrived. |
| Healthy OpenCode | **Pass** | Tab `befefab0` logged one `launch_ready` event with `ready_idle`, confidence `0.880`, episode 1 at `00:16:57Z`; the parent received one `spawn-ok`. No action was emitted. |
| Bad-model Codex | **Fail** | Tab `68b56f5e` visibly returned HTTP 400 for `no-such-model-zzz`; tab inspection showed `runtime_session_model_mismatch`. It logged stale discards at `00:17:39Z` and `00:19:39Z`; no tab-state `model_unavailable` signal or action appeared. |
| Closed-proxy Claude | **Fail** | Tab `068d5a7c` visibly showed `ECONNREFUSED` retries. It logged stale discards at `00:17:50Z` and `00:19:50Z`; no tab-state `error_loop` signal or action appeared. |
| Logged-out Codex home | **Pass for the informational hint; escalation unproven** | Tab `40221cd2` showed unanswered sign-in choices. At `00:19:59Z`, one `startup_deadline` classified `login_or_auth_required` at confidence `1.000`, episode 1. No second agreeing strong observation or tab-state action was seen by `00:21:54Z`. |
| Fresh-folder trust | **Fail / incomplete** | Tab `a4d82428` showed the unanswered DirectoryTrust choice. No tab-state trust classification appeared by `00:21:54Z`. A separate worker startup report requested parent inspection with `action_needed=true`; the Orchestrator inspected only and sent no response. |
| Killed provider OpenCode | **Fail for the tab-state exit signal; direct-kill recovery observed** | On tab `ca32a1a2`, provider PID `6990` was confirmed in that tab's process tree as the `opencode` child of shell PID `6983`, then only PID `6990` received `SIGTERM`. The lifecycle recorded `provider process exited to a surviving shell`; the screen showed the surviving shell. No `agent restart` or post-kill tab-state crash classification/action occurred. |

### Cross-case oracle checks

- **Healthy spawn-ok:** 1 of 3 healthy PTY providers passed.
- **Two agreeing strong observations before action:** not demonstrated. Login produced one strong observation and no tab-state action; trust produced no tab-state observation. No pair of agreeing strong observations was recorded. The separate worker-startup `action_needed` advisory on the trust prompt was not a tab-state action; it led only to inspection, with no prompt response.
- **No action on a weak hint:** no weak hint and no tab-state action appeared. The weak-hint path itself was not exercised.
- **Proxy and bad-model signals:** visible ground truth was present in both cases, but neither tab-state classifier signal was delivered.
- **Recommendation:** keep automatic actions and hints off as the historical shipping defaults. This run does not validate the required healthy coverage or the two-observation gate.

### Evidence and limits

The observations above come from `~/.codeterm-dev-tabstate/logs/codeterm.log`, read-only tab lifecycle inspection, and visible terminal text. The stale-discard records for Claude/Codex remain on the build containing the merged spawn-output fix; OpenCode emitted `ready_idle` on that same build. The proxy and bad-model screens were present while their tab-state observations were discarded. This run establishes the behavior, but does not isolate why the stale checks remain provider/timing dependent.

The temporary Team was closed, the `tabstate` instance was stopped, its action flags were reset, and the temporary provider wrappers, fixture directories, context files, and ignored capture files were removed. The `7697` health endpoint refused connections afterward; production on `7685` remained healthy.

## Previous run — 2026-10-01 baseline

### Verdict

**Fail.** The runtime did not meet the e2e oracle. The healthy PTY launches produced no `spawn-ok` line because their spawn observations were discarded as stale. The closed-proxy and bad-model screens also produced no matching signal. Auth and trust each produced one strong startup-deadline classification, but neither produced a second agreeing observation. The auth hint was followed by an Orchestrator `action_needed` notification before a second strong observation. No prompt was answered.

### Run identity and safety

- Canonical source: `release/1.12.3`, fetched from origin at `fb2a932162cd347d705552b88d5c53a8df9623cb`.
- Built instance: `tabstate`, API port `7697`; build reported `fb2a9321`.
- Temporary Team: `team_624ff114`; Plan: `pl_bbsrm7`; Orchestrator tab: `107fc099`; Planner tab: `610fdcdb`.
- The Team was closed and the instance was stopped after collection. All oracle tasks were marked cancelled during cleanup.
- The only user prompts observed were Codex sign-in and Claude folder trust. Neither received input; no `tab respond` command was run.
- LM Studio's local model endpoint was reachable and the official `lmstudio` plugin was installed. Opening its chat tab failed with `chat_backend_session_config_required`; no chat transcript tab was created.
- Cursor exited with code 1 before launch readiness; CodeTerm rolled its tab back. No screen or observer call remained to capture.

### Commands used

Commands below omit the token value; the token was only piped from the authorized local file into `dc` stdin. Bench calls ran under Bash with the target instance exported before sourcing the helper. A first zsh source attempt failed on `BASH_SOURCE` before issuing a `dc` command. A helper call without the target variables also failed resolution before issuing a `dc` command.

```bash
git fetch origin release/1.12.3
CODETERM_INSTANCE=tabstate CODETERM_API_PORT=7697 ./build.sh dev --wait

# Earlier setup on the same instance; token never printed.
python3 -c "import json,sys; sys.stdout.write(json.load(open('/Users/dev/.codeterm/decision-models/typesafe-jev.json'))['token'])" | dc supporter token set
dc supporter status
dc supporter use typesafe-jev
dc plugin install lmstudio

# All dc and capture-helper calls below used this instance binding.
export CODETERM_INSTANCE=tabstate CODETERM_API_PORT=7697
source evals/_lib/lib.sh

dc agent providers --json
dc team pair --orchestrator claude --planner claude --workspace a1fa0ad0 --title "Tab State Oracle" --acknowledge-leaf --json
dc --as-tab 107fc099 plan create --title "Tab State Oracle" --project "$HOME/Developer/codeterm" --json
dc --as-tab 107fc099 team adopt-plan team_624ff114 pl_bbsrm7
```

The Team Plan contained one task for each of these cases: `healthy_claude`, `healthy_codex`, `healthy_opencode`, `healthy_cursor`, `healthy_lmstudio`, `bad_model_codex`, `closed_proxy_claude`, `logged_out_codex_home`, `fresh_folder_trust_claude`, and `killed_provider_opencode`. Dispatches used `plan task dispatch` from the Orchestrator tab with `--execution-intent read_only`; examples:

```bash
dc --as-tab 107fc099 plan task dispatch pl_bbsrm7 task_0szl0f --agent claude --parent-tab 107fc099 --execution-intent read_only --json
dc --as-tab 107fc099 plan task dispatch pl_bbsrm7 task_za0uaq --agent codex --parent-tab 107fc099 --execution-intent read_only --json
dc --as-tab 107fc099 plan task dispatch pl_bbsrm7 task_9zguzi --agent opencode --parent-tab 107fc099 --execution-intent read_only --json
dc --as-tab 107fc099 plan task dispatch pl_bbsrm7 task_vjg09u --agent cursor --parent-tab 107fc099 --execution-intent read_only --json
dc --as-tab 107fc099 plan task dispatch pl_bbsrm7 task_r3z8t6 --agent lmstudio --parent-tab 107fc099 --execution-intent read_only --json
dc --as-tab 107fc099 plan task dispatch pl_bbsrm7 task_a1td85 --agent codex --parent-tab 107fc099 --execution-intent read_only --cwd "$HOME/Developer/codeterm" --json
dc --as-tab 107fc099 plan task dispatch pl_bbsrm7 task_kchyq5 --agent claude --parent-tab 107fc099 --execution-intent read_only --cwd "$HOME/Developer/codeterm" --json
dc --as-tab 107fc099 plan task dispatch pl_bbsrm7 task_i55xgm --agent codex --parent-tab 107fc099 --execution-intent read_only --cwd "$HOME/Developer/codeterm" --json
dc --as-tab 107fc099 plan task dispatch pl_bbsrm7 task_gyf434 --agent claude --parent-tab 107fc099 --execution-intent read_only --cwd /tmp/tabstate-trust.bUMpH9 --json
dc --as-tab 107fc099 plan task dispatch pl_bbsrm7 task_lgdled --agent opencode --parent-tab 107fc099 --execution-intent read_only --json
```

Case setup used per-instance CLI wrappers in `$HOME/.codeterm-dev-tabstate/bin`, restricted to the test working directory: Claude received `HTTPS_PROXY=http://127.0.0.1:9` for the proxy case; Codex received an empty `CODEX_HOME` for the logged-out case; for the bad-model case a temporary Codex wrapper rewrote the provider's valid `--model` value to `no-such-model-zzz` after CodeTerm accepted the catalogue model. Those wrappers and both temporary directories were removed after the run. These wrappers were test setup only; they did not change repository source.

The proxy wrapper invoked `/Users/dev/.local/bin/claude` with the closed-port proxy only when `$PWD` was the canonical repository. The logged-out wrapper invoked `/opt/homebrew/bin/codex` with `CODEX_HOME=/tmp/tabstate-codex-home.0RmIdJ` under the same cwd condition. The bad-model wrapper transformed either `--model VALUE`, `-m VALUE`, or `--model=VALUE` into the same flag with value `no-such-model-zzz`, then invoked `/opt/homebrew/bin/codex`; it passed all arguments through unchanged in other working directories. The `mktemp -d` directories were `/tmp/tabstate-codex-home.0RmIdJ` and `/tmp/tabstate-trust.bUMpH9`.

The LM Studio task dispatch and direct `agent spawn` attempts with the `codeterm` preset and Jev-style model failed with the same `chat_backend_session_config_required` response. No further provider configuration was changed.

Capture and label commands followed this form, with context files in the private agent pad and every label assigned by Watchman after reading the screen:

```bash
CODETERM_INSTANCE=tabstate CODETERM_API_PORT=7697 python3 evals/tab-state-jev/capture_screen.py capture --tab <tab-id> --context-file <pad-context.json> --note '<screen ground truth>'
CODETERM_INSTANCE=tabstate CODETERM_API_PORT=7697 python3 evals/tab-state-jev/capture_screen.py label --id <capture-id> --label <human-label> --by watchman
```

The provider-kill case used the authorized CLI force-restart operation. It killed the old PTY and immediately resumed OpenCode in a new PTY in the same tab; it did not leave a shell screen:

```bash
dc agent restart --force --grace-secs 0 --tab 21393286 --json
```

Cleanup and shutdown:

```bash
for task in task_0szl0f task_za0uaq task_9zguzi task_vjg09u task_r3z8t6 task_a1td85 task_kchyq5 task_i55xgm task_gyf434 task_lgdled; do
  dc --as-tab 107fc099 plan task update pl_bbsrm7 "$task" --status cancelled --why "Oracle run ended; observations are in the transcript" --json
done
dc team close team_624ff114 --json
CODETERM_INSTANCE=tabstate CODETERM_API_PORT=7697 ./build.sh kill
```

The final capture helper `pending` query returned no unlabelled frames. No seed `frames.json` or `results.json` file is included in this transcript commit.

### Per-case results

| Case | Tab / visible ground truth | Runtime observation and Orchestrator delivery | Expected vs observed | Calls |
|---|---|---|---|---:|
| Healthy Claude | `18248b1b`; `ready_idle`; capture `live-claude-5a5762c6be` | `stale observation discarded`; no `spawn-ok` or action | Expected one `spawn-ok`, no action; observed neither | 1 |
| Healthy Codex | `20f49fa0`; `ready_idle`; capture `live-codex-6ef6666727` | `stale observation discarded`; later spawn triggers skipped as already observed; no `spawn-ok` or action | Expected one `spawn-ok`, no action; observed neither | 1 |
| Healthy OpenCode | `64b7df7a`; `working_normally`; capture `live-opencode-eb56937eaa` | `stale observation discarded`; no `spawn-ok` or action | Expected one `spawn-ok`, no action; observed neither | 1 |
| Healthy Cursor | `ffc0dfa2`; provider process exited before readiness (`exit_code=1`), tab rolled back | No observer call; no retained screen | Expected one `spawn-ok`, no action; launch did not reach readiness | 0 |
| Healthy LM Studio | no tab; local endpoint reachable | Dispatch and direct spawn attempts returned `chat_backend_session_config_required` | Expected one chat transcript tab and one `spawn-ok`; no tab was created | 0 |
| Bad model | `29712055`; Codex displays HTTP 400 unsupported `no-such-model-zzz`; capture `live-codex-b206e95773` | Spawn observation was discarded stale; later trigger skipped as already observed; no model-unavailable event | Expected a first hint and action only after a second agreeing strong observation; observed no classifier signal | 1 |
| Closed proxy | `e42bfd5b`; Claude displays connection-refused retry loop; capture `live-claude-bf02708b1c` | Spawn observation was discarded stale; later trigger skipped as already observed; no error-loop event | Expected a first hint and action only after a second agreeing strong observation; observed no classifier signal | 1 |
| Logged-out CODEX_HOME | `bfbc4015`; Codex sign-in choices visible and unanswered; capture `live-codex-cdf7d264a1` | Startup deadline classified `login_or_auth_required`, confidence `1.000`, episode 1. Orchestrator received one `Tab-state hint` for this tab. It then sent `action_needed` notification `b2c66279`; no second strong observation was logged. | Expected first hint, then action after a second agreeing strong observation; observed one strong classification/hint followed by an owner notification, with no second observation | 1 |
| Fresh-folder trust | `929bb4a6`; Claude DirectoryTrust choices visible and unanswered; capture `live-claude-871d99b9e1` | Startup deadline classified `trust_or_permission_prompt`, confidence `1.000`, episode 1. No second observation or matching Orchestrator hint was visible. The Orchestrator had already sent `action_needed` notification `ed2d4d9f` in response to the Worker waiting report, before this classifier result. | Expected first hint, then action after a second agreeing strong observation; observed one classification and a separate earlier report-derived notification | 1 |
| Killed provider | `21393286`; OpenCode was `working_normally` before the forced restart (`live-opencode-5d4488ce29`) and working after restart (`live-opencode-644ee06b39`) | Initial spawn result discarded stale. The forced restart reaped the old PTY and resumed a new one. Startup deadline later classified `login_or_auth_required`, confidence `0.430`, episode 2; no crash-to-shell event or shell ground truth appeared. | Expected crash-to-shell hint/action after two agreeing strong observations; observed no crash signal and no surviving shell frame | 2 |

**Jev calls used: 9.** Counted completed observer calls from the per-tab `stale observation discarded` and classifier-result log lines. Skipped `SpawnAlreadyObserved` and suppressed early-startup entries are not calls. Calls by group: 3 healthy PTY launches, 4 broken cases with one call each (bad model, proxy, auth, trust), and 2 calls for the OpenCode kill/restart case. Cursor and LM Studio produced no calls.

**Healthy spurious actionable signals: 0.** The three healthy PTY tabs that launched had no actionable signal, but also missed the required `spawn-ok`. Cursor and LM Studio did not reach a tab state that could be evaluated.

### Evidence and limits

**Executed results:** `/tmp/codeterm-dev-tabstate.log` records stale discards for the three healthy PTY tabs and the proxy, bad-model, and initial OpenCode observations. It records the single strong auth and trust classifications and the weak-confidence OpenCode restart classification. The Orchestrator screen showed the auth hint and notification ID `b2c66279`; the trust notification ID `ed2d4d9f` appeared before its classifier result. Screen captures above are from `capture_screen.py` and were human-labelled with `--by watchman`.

**Source-level interpretation:** the repeated stale discards happen immediately after launch session binding changes. This points to the spawn observation retaining a pre-binding identity and being rejected when the model call returns. This is an inference from the timestamp order; no source fix was attempted in this oracle run.

**Not proven:** there was no successful `spawn-ok`, no second agreeing strong observation, no model/proxy/crash action, no Cursor healthy session, and no LM Studio transcript session. The run therefore does not validate the end-to-end acceptance criteria.
