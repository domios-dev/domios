# Lane checks

Run the harness from one checkout: `<HARNESS>/scripts/aiderrun`. During review HARNESS is `D:/Developer/codeterm-worktrees/aid1-bp-harness`; after Mozart's real merge it is `D:/Developer/codeterm-worktrees/dev-aiderrun`. Worker lanes are read-only inputs to the harness. Never copy harness files or runtime state into them. Runtime state is gitignored at `<HARNESS>/scripts/aiderrun/.runtime/<AIDER_TAB>/`.

Entrypoints:

```powershell
node <HARNESS>/scripts/aiderrun/lane-check.mjs --lane '<LANE>' --worker '<AIDER_TAB>' --orchestrator b3a2c8ff --surface '<SURFACE>'
npx tsx scripts/aiderrun/lane-check.test.ts
```

All launched shells explicitly belong to `team_dd6d7d99`. The launcher resolves the installed dev CLI from CODETERM_CLI_PATH or the instance-prepended PATH, requires dev port 7686, and uses fixed Mozart caller identity. No evals, older bash CLI, Rust gate, daemon restart or rebuild is involved.

## Exact binding

Verify the exact tab's session and adapter_start processGeneration, then configure identity once:

```powershell
node <HARNESS>/scripts/aiderrun/bind.mjs --lane '<LANE>' --worker '<AIDER_TAB>' --orchestrator b3a2c8ff --surface '<SURFACE>' --session '<SESSION>' --generation '<PROCESS_GENERATION>'
```

The installed Aider adapter writes `<LANE>/.aider/history/<SESSION>.md.domios-turns.jsonl`. Bind only the claimed session; never select a newest file. Probe reads a bounded sidecar head/tail and exact UTF-8 user-record byte ranges. It ignores partial outcome records and blank user records, exposing the newest meaningful completed outcome. Blank submits leave its key unchanged. Only an outcome tail matching current historyBytes permits admission; newer unfinished user input closes it. Nonblank errors/cancellation yield failure. Unknown identity fails explicitly.

## Mozart activation

Prepare handlers.json, replacing every placeholder. Quote shell arguments with the host-platform rules implemented by runtime.shellCommand:

```json
{"waiting":[],"completed":[{"id":"launch-team-lane-check","type":"command","parameters":{"command":"node '<HARNESS>/scripts/aiderrun/launch.mjs' --lane '<LANE>' --worker '<AIDER_TAB>' --orchestrator b3a2c8ff --surface '<SURFACE>'","label":"Launch Team lane utility","timeout_ms":9000}}],"failed":[{"id":"report-source-failure","type":"notify","parameters":{}}]}
```

Mozart runs this first with --dry-run, reviews the normalized definition, then removes only --dry-run:

```powershell
codeterm blueprint create --name lane-check-<AIDER_TAB> --probe "node '<HARNESS>/scripts/aiderrun/probe.mjs' --lane '<LANE>' --worker '<AIDER_TAB>' --orchestrator b3a2c8ff --surface '<SURFACE>'" --question exact-adapter-completion --buckets waiting,completed,failed --match 'waiting=(?m)^waiting$' --match 'completed=(?m)^completed ' --match 'failed=(?m)^failed ' --rearm-on-change completed --handlers-file '<HANDLERS_ABSOLUTE_PATH>' --every 10s --timeout 4h --max-transitions 256 --max-actions 256 --dry-run
```

Record graph id with exact worker/lane/surface/session/generation. Mozart owns all accepted graphs; stop them when their task ends. The disposable proof uses 20m/8/8. No tsc or tests run inside a Blueprint probe/action: the fixed command only creates a Team utility and returns.

## Checks and containment

| Surface | Commands after common `npx tsc --noEmit` |
| --- | --- |
| config, defaults | `node scripts/run-ts-unit-tests.mjs src/hooks/useConfig.test.ts` |
| sdk | `npx tsc --noEmit --strict --skipLibCheck --target ES2021 --module ESNext --moduleResolution bundler packages/plugin-sdk/src/types.test.ts` |
| shortcuts | `node scripts/run-ts-unit-tests.mjs src/help/shortcuts.test.ts` |
| automation | `node scripts/run-ts-unit-tests.mjs src/components/Automation/nodeDetailsModel.test.ts src/components/Automation/CommandResultDetails.test.tsx src/components/Automation/AutomationGraphView.test.tsx` |

Commands run in the exact worker lane. Missing node_modules or any selected target fails explicitly with its path; unknown surfaces fail. The whole command sequence has a 180-second failure deadline, plus five seconds for cleanup settlement. Windows uses encoded PowerShell argument transport and taskkill /PID /T /F; Unix uses a detached process group and SIGKILL. Unconfirmed containment retains active.lock and produces FAILED.

The launcher persistently claims a turn before tab creation. Duplicate triggers and active locks create no additional utility. A failed or uncertain launch is fenced against blind retries. The runner persistently claims the turn before checks/routing. HEAD, binary diff and actual untracked contents form its fingerprint. An unchanged fingerprint suppresses verdict-response feedback; a later completed turn with new contents rearms even if pass/fail is unchanged. The first meaningful observed outcome runs one initial check.

Verdict.txt is UTF-8 with PASSED/FAILED first, then exact run identity and diagnostics. Both recipients receive CONTENT through dev CLI send --from-pad, never a result pointer. Routing.json retains each outcome; unconfirmed sends never replay. A crash retains the lock as a fence. Mozart must verify exact process settlement before any manual recovery; a deadline does not authorize retries or a higher cap.

## Verification status

Initial workspace dependency provisioning caused missing SDK errors; Mozart repaired pnpm installs and the exact lane tsc now exits 0. Focused entrypoint passes 59 assertions. Affected-tests selects no tests under this directory, so invoke the entrypoint explicitly. Earlier disposable graph proved a Team utility launch in 2881ms with actual exit 0 and resources confirmed. The live Probe2 utility ran successful config checks (both exits 0, containment confirmed); verdict creation occurred 42 seconds after launcher admission, outside the ten-second action cap. A real Windows cancellation fixture exits nonzero at its three-second deadline with containment confirmed. Aider consumption and live edit/rearm remain blocked by core managed-tab refusal, escalated to Fermi. Deliberate failure/recipient-removal and no-replay behavior have injected coverage; a live failure and removed-recipient drill remain unexecuted. Verdict sends use a 30-second failure deadline; passing command output is omitted, failing diagnostics remove PowerShell progress CLIXML, and each recipient result persists immediately. Unconfirmed routing never triggers automatic replay. Fixtures and source facts alone do not establish working Aider hands. Dispatch stays gated until Mozart accepts that evidence.

