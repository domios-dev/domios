# Live model-switch acceptance

This opt-in project attaches to DEV 7686/CDP 9223. It never starts, stops,
resets or restarts DEV. Run only under the Orchestrator's serialized QA authority.
Provider credit is required. Owner tabs are outside the test scope.

Install this checkout's dependencies with `pnpm install --frozen-lockfile`.
The product SHA and test-source SHA can differ; `PW_SWITCH_DEV_SHA` must pin
the running product's full SHA, supplied by the Orchestrator. The standard
fixture verifies DEV identity and that exact SHA before attaching.
`CT_E2E_DEV_ROOT` names the running canonical checkout for its frontend
build-identity file and bench library. The test and record-reader code still
come from the suite checkout.

PowerShell, after the Orchestrator supplies DEV UP and its SHA:

```powershell
$env:PW_TARGET = 'dev'
$env:PW_SWITCH_DEV_SHA = '<full DEV SHA>'
$env:CT_E2E_DEV_ROOT = 'C:/Developer/codeterm/codeterm'
$env:PW_SWITCH_PROVIDERS = 'claude,codex,grok'
$env:PW_SWITCH_RUNS = '3'
$env:PW_SWITCH_KEEP = '1'
pnpm exec playwright test -c tests/playwright --project=live-model-switch
```

Include `opencode` only after the Orchestrator confirms its current hook is on DEV.
For a coordinated single run, set `PW_SWITCH_RUNS=1` and
`PW_SWITCH_RUN_NUMBER` to its stability index (1..3). Optional
`PW_SWITCH_WORKSPACE` must name an empty provisioned `SwitchQA-*` workspace;
the suite validates it before spawning tabs. Clear that variable for automatic
workspace creation or when using a restored manifest.
For an explicitly scheduled scenario rerun, `PW_SWITCH_STEPS=busy` runs baseline
and busy only. Clear it before full acceptance; the result records requested
steps so a focused scenario pass cannot be mistaken for a full matrix pass.
Descriptors prefer a pair with two explicit effort options per model, retaining
the provider default when it belongs to that pair. Otherwise they use the default
and a different primary model, or another catalogue model. No model
ids are hardcoded. Restart/resume modes fail with their explicit descriptor
value; this acceptance path requires live or next-turn switching.

Each matrix creates a provisioned `SwitchQA-*` workspace and one fresh tab per
provider through `evals/_lib/lib.sh` and `dc`. Composer sends and status-line
Apply use the real UI. All selectors are scoped to the created tab. The suite
checks idle, effort, combined, busy and back switches. Unsupported effort
capability is explicitly recorded. A missing evidence field is a failure.

Claude command wrappers are excluded from real user-turn counts only when
`src/lib/chatPrefixes.ts::detectChatPrefix` identifies a command origin with
an empty remaining body. This follows the complete-wrapper exclusion in
`src-tauri/crates/codeterm-plugin/src/provider/model_switch_transcript.rs`;
text outside a wrapper remains user input. Raw records are retained, and
rendered slash-command user bubbles are forbidden separately.

Evidence comes from the bound session's provider storage:

- Claude: assistant `message.model` and top-level `effort` following the marked user.
- Codex: retained user `turn_id` joined to that turn's `turn_context.model/effort`.
- Grok: marked user model metadata plus assistant-linked `turn_completed`
  model usage. Native `-build` model variants must agree with the configured id.
  Effort uses explicitly labeled session-level evidence: the provider-authored
  exact-session `summary.json` must be updated after Apply, read before the ACK
  starts, and carry the selected effort. It is not labeled a turn-record proof.
- OpenCode: exact session SQLite rows and completed parent-linked assistant
  `providerID/modelID`, with matching answer. Effort is labeled
  `turn-config evidence (user.variant)` from that assistant's exact parent user.

Every action has a 15-second failure cap. Turn and streaming waits use DOM
or SSE events with bounded deadlines. No sleeps, injected responses, network
intercepts or provider fakes are used. Retries are disabled.
Scoped SSE `lagged` frames trigger exact own-tab status and delivery-ledger
resync, adopt the supplied replay cursor and continue live frames. Resyncs are
recorded; a current Idle snapshot cannot manufacture a past Working transition.
Unexpected DEV boot changes remain failures outside the explicit restored leg.
Busy completion re-reads exact session records on filesystem changes and own-tab
status/chat SSE notifications, bounded at 240 seconds. Claude requires the marked assistant's `end_turn`;
Codex requires the exact turn's marked `task_complete`; Grok and OpenCode retain
their completed-record oracles. A transient Idle never proves completion.

Artifacts live in `artifacts/model-switch-<timestamp>/`: matrix JSON, raw
records, screenshots and traces. Each result includes DEV SHA, provider, step,
mode, picker-close time, settle/pending time and record confirmation time.
The DOM observer records the actual visible Applying duration separately from
runner timing, or its lower bound when the step fails while it remains visible.
Before failure cleanup, own-tab PTY text, inspection, transcript and DEV log
lines capture delivery events and `[agent-model/timing]` phases. Set
`PW_SWITCH_DEV_LOG` if the DEV log differs from `~/.codeterm-dev/logs/codeterm.log`.
Claude/Grok idle settling must finish under 3 seconds; next-turn pending must
appear within 1 second and exact records confirm on the following ACK.

With `PW_SWITCH_KEEP=1`, the final workspace remains and its
`restore-manifest.json` names the exact scratch tabs. Send its path to the
Orchestrator and request one DEV restart. After DEV UP:

```powershell
$env:PW_SWITCH_DEV_SHA = '<new full DEV SHA>'
$env:PW_SWITCH_RESTORE = '<absolute restore-manifest.json path>'
pnpm exec playwright test -c tests/playwright --project=live-model-switch
```

The restored leg requires the same session and new launch nonce/PTY, then runs
the matrix again without spawning replacement tabs. It closes only the recorded
scratch workspace. A failed cleanup is reported, never redirected to prod.
The project is absent from default project discovery and excluded from the
ordinary live project; explicit selection enables it in runner and workers.

Source checks:

```powershell
pnpm exec tsx tests/playwright/helpers/live-model-switch.test.ts
$env:PW_LINT_SCOPE = 'tests/playwright/suites/03-agents/live-model-switch.spec.ts'
pnpm test:pw:lint
pnpm exec tsc --noEmit -p tests/playwright/tsconfig.json
```

Report each matrix to Fermi with its timing table, failures, artifacts and
DEV SHA. An instrumentation failure is not a product repro or stability run.
Product failures carry the provider, picker selection, exact records and
observable UI result. Product repairs belong to the Orchestrator's other lanes.
