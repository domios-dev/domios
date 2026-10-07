# Codex remote TUI gate

## Final Windows verdict: GO for lane A under the accepted P1 adaptation

The Orchestrator accepted Windows GO after reviewing the transcripts, the hot-resume binding finding and the carried risks. This is a lane-A admission decision under the following P1 contract, not a claim that the original thread/started-only rule passed:

- Startup binds with `thread/start -> thread/inject_items (short developer marker) -> TUI remote resume`, using the same absolute native binary.
- `/fork`, `/clear` and `/new` follow broadcast `thread/started`.
- Hot `/resume <id>` additionally follows the first turn/item event whose threadId differs from the bound root. P1 must verify that candidate with `thread/read` as a top-level user thread before any structured send. Reject subagents using threadSource, parentThreadId and source.subAgent, and reject title-worker sources. Until verification the sidecar sends nothing.
- P1/P2 persist approval outcomes from the live stream to cover denied-command backfill omissions. The Windows busy-after-interrupt behavior remains a provider UI defect. Instruction/hook-trust argv-versus-thread precedence, selective hook disabling and exhaustive shared-daemon isolation remain P1 live-oracle work.

The rebinding adaptation is an **explicit Orchestrator design decision for P1**, not product code implemented or independently verified by this spike. P1 must prove its hold/verify/send ordering. Windows resize, subagent discrimination, all four visible slash transitions, second-client streaming/control and owned-descendant teardown have executed evidence below. Both Windows and macOS physical-machine transcript locations are recorded; macOS results are attributed to Fulton.

All remaining Windows surfaces were exercised on Codex 0.160.0. Resize passed; real subagent source/parent discrimination passed; normal utility-tab teardown left no observed descendant survivors. `/fork`, `/clear`, `/new` and `/resume <id>` all changed the visible TUI as intended. However, **item 2 fails its broadcast binding oracle for hot resume**: returning to the still-loaded original thread emitted no new `thread/started` to either connected probe. The TUI restored SEED_READY history, while the most recent broadcast user thread remained the `/new` thread. A sidecar rebinding only on `thread/started` would therefore send to the wrong thread before any subsequent TUI input. Only `thread/goal/cleared` for the original thread was observed around this resume; it is not an established selection contract.

This finding initially produced a NO-GO for the original rule. The subsequent Orchestrator decision above accepts the P1 adaptation and changes the final Windows lane-A verdict to GO. The missing broadcast remains an observed fact; it has not been fixed or relabeled PASS. No product repair was written. Fulton's macOS evidence remains referenced below.

### Final Windows slash and teardown evidence

Utility tab `906c2347`, temp directory `C:/Users/dev/AppData/Local/Temp/codex-remote-spike-slashes-20261007`, same native absolute binary for server/TUI, port 18471. A seed turn materialized the original thread; this final run tests slash selection, while the earlier injection run proves the recommended startup binding order. The stale restored tab `9d68c34c` was closed before continuing.

| Action | Visible outcome | Broadcast/identity outcome |
|---|---|---|
| /fork | Fork created; seed history retained | `thread/started`, ID `01a11651-4d80-70b0-be92-991f2d05573e`, threadSource user, forkedFromId original |
| /clear | Empty fresh conversation | `thread/started`, ID `01a11651-dfe8-7233-80ec-6f8b30e30bca`, threadSource user |
| /new | Empty fresh conversation | `thread/started`, ID `01a11652-11cd-78b3-9dc5-478bc0121e0e`, threadSource user |
| /resume original ID | Restored SEED_READY history | No new `thread/started`; original `01a1164f-f8ac-7102-9785-6f9a7c2c7ad3` remained loaded/subscribed |

Raw frames: [windows-slash-mint.ndjson](evidence/windows-slash-mint.ndjson), [windows-slash-observer.ndjson](evidence/windows-slash-observer.ndjson). Screens: windows-slash-{fork,clear,new,resume}-screen.txt. Resume's explicit ID avoids any newest-thread heuristic or picker ambiguity. The observer independently read the earlier real child `01a1164a-9049-7490-ab67-7ed8f6e71069`: `threadSource:"subagent"`, parentThreadId and forkedFromId equal `01a11649-8af5-73e3-b620-f575ebc7c1fb`; `source.subAgent.thread_spawn` carries parent_thread_id, depth 1, agent_path `/root/oracle_child` and nickname Descartes. This confirms the source discriminator in addition to the live subAgentActivity linkage.

Commands used the canonical bench lib as requested:

```bash
source D:/Developer/codeterm/evals/_lib/lib.sh
export MSYS2_ARG_CONV_EXCL='*'
dc send --tab 906c2347 --actions '/,f,o,r,k,enter'
dc tab get-text --tab 906c2347
dc send --tab 906c2347 --actions enter
```

The first Enter completed the slash command in the composer; a separate inspected submit executed it. `/clear` and `/new` used the corresponding literal keys and the same inspected submit. `/resume 01a1164f-f8ac-7102-9785-6f9a7c2c7ad3` was written from the AgentPad with `dc send --from-pad - --actions enter`. Git Bash's default slash-argument path conversion caused the initial key-only attempt to be refused before input; `MSYS2_ARG_CONV_EXCL='*'` prevents that bench-shell trap. No delay, payload replay, blind prompt choice or newest-session selection was used.

Teardown: launcher PID 50200, server PID 15004, TUI PID 20492, minter/MCP/conhost descendants were recorded by native parent closure. `dc tab close --tab 906c2347` returned ok true; [windows-slash-audit.json](evidence/windows-slash-audit.json) records zero survivors and pass true. No port 18471 listener remained; residual temp token removed explicitly. Both spike utility tabs are closed. An audit harness precision bug was corrected: converting a JSON-parsed DateTime back through text lost fractional ticks and falsely classified live PIDs as reused. A same-live-identity regression now retains two live processes with zero exclusions; post-teardown verification returns zero. This was an evidence-tool fix, not product code or a changed provider result.

Validation: both Node syntax checks exit 0; native audit live-identity and PID-reuse checks pass; final descendant verify exit 0, seven previously observed processes and zero survivors. Project gateway verification remains deferred to canonical under the source-only Worker rule. Item 2's missing hot-resume broadcast is addressed by the accepted P1 contract at the top; item 11's gap remains carried to live-stream outcome persistence.

## Resumed verification and accepted risks

The Orchestrator accepted the denied-command backfill gap as non-blocking for lane A: P1/P2 must persist approval outcomes from the live stream. The TUI busy-after-interrupt discrepancy is a recorded provider UI defect. Argv-versus-thread instruction/hook-trust precedence, selective hook disabling and exhaustive shared-daemon isolation carry into P1's live oracle. The original full-checklist verdict below records the earlier state; the final Windows lane-A decision is at the top.

macOS verification was completed by Fulton on eight, using native arm64 Codex 0.160.0 at `/opt/homebrew/lib/node_modules/@openai/codex/node_modules/@openai/codex-darwin-arm64/vendor/aarch64-apple-darwin/bin/codex`, from `/Users/dev/Developer/codeterm-worktrees/codex-hybrid-macqa` at lane tip `2b000075`. The Orchestrator's evidence stride on `pl_a0rcq7/task_8ghd29` records injection binding, slash transitions (/fork /clear /new /resume), subagent discrimination through thread/read, second-client streaming, clientId echo, approval fan-out both ways, interrupted turn with usable TUI, next-turn model and owned-process teardown. The same denied-command backfill gap was observed.

The uncommitted macOS evidence bundle is `/Users/dev/.codeterm/runtime/agent-pads/20927f0a/hybrid-evidence/` on eight: per-flow launch.json, mint.ndjson, observer.ndjson, terminal screens, PID receipts, and teardown.txt containing every PID/token/listener and thread ID. These are **Fulton's executed results relayed through the Orchestrator**, not independently rerun by this Windows Worker. Mac evidence remains on eight and is not copied into this git lane. The plan stride and physical-machine transcript paths are the review references.

### Windows final observations before the second restart

The resumed owner DEV run used utility tab `9d68c34c`, launcher PID 5944, server PID 16564 and TUI PID 44304. `dc identify` reported CLI SHA `30170071c9cc73cc332a54a2e456954dd0075a30`; this is the CLI build receipt, not an independently verified daemon SHA. Codex remained native 0.160.0. Same injection flow, dedicated port 18471, fresh temp directory `C:/Users/dev/AppData/Local/Temp/codex-remote-spike-final-windows-bfbf99400cf845008a811446329fbc33`.

Resize **PASS**: sourced the bench library and called `dev_post /api/tabs/9d68c34c/resize '{"cols":80,"rows":24}'`, then the same endpoint with 120/40; both returned `{"ok":true}` and the TUI visibly reflowed/truncated its cwd/footer at narrow width and restored them at wide width. Screens: windows-final-resize-80x24.txt and windows-final-resize-120x40.txt.

Real subagent activity **OBSERVED**: root `01a11649-8af5-73e3-b620-f575ebc7c1fb` spawned child `01a1164a-9049-7490-ab67-7ed8f6e71069`; parent-stream `subAgentActivity` items explicitly carry `agentThreadId` and `agentPath:"/root/oracle_child"`. Child completed with CHILD_ORACLE; parent completed with PARENT_ORACLE. The observer stayed bound to the root. A different broadcast `thread/started` was an ephemeral title worker, `01a1164a-80f5-71e3-a762-67c88aeaee88`, with `threadSource:"thread_title"`; thread/read confirmed it. ParentThreadId alone would incorrectly admit this title worker because it is null. Filter explicit subAgentActivity agentThreadIds and non-user threadSource values; do not rebind to title threads. The actual child's thread/read and slash transitions were interrupted before observation, so this is partial discrimination evidence, not a completed slash-switch oracle.

During the /fork step the host turn was interrupted by a PROD restart/restoration message. After restoration, DEV was unavailable and the owned launcher/server/TUI had exited. No normal slash transition result is claimed. Frames and composed environment: windows-final-pre-restart-{observer,mint}.ndjson and windows-final-pre-restart-launch.json.

Descendant/orphan audit **PASS for the interrupted run**: native Toolhelp snapshots at launch and after subagent completion tracked launcher, both Codex processes, mint client, MCP node and conhost descendants (seven live processes). Post-restart verification found zero survivors and no 18471 listener. Windows reused PID 51260 for an unrelated process; creation-time comparison excluded it, and it was not touched. See windows-final-pre-restart-audit.json. The residual temp auth token was removed explicitly. CIM process-event access was denied by the OS, so the audit uses explicit-phase snapshots and creation identities; it does not claim a continuous process-start trace. The subsequent slash and child-source observations are recorded in the final section above.

## Historical full-checklist verdict (before carried-risk decision)

**Windows: NO-GO for the complete checklist. macOS: NOT OBSERVED (NO-GO for release until verified). Lane A remains blocked.**

The initial empty-thread resume failure is solved by materializing history. The recommended, verified Windows binding order is `thread/start -> thread/inject_items (short developer marker) -> TUI --remote ... resume <id>`. The marker is hidden in the TUI and the first TUI turn streams to a subscribed probe. A seed model turn also works; TUI-created threads can be joined after their first turn. These successful flows replace the initial binding-failure verdict.

Remaining concrete Windows failure: item 11, full backfill. Two declined/canceled approval commandExecution items observed live are absent from paginated thread/items/list. The returned IDs are stable across repeated reads, but full-history inclusion is false. Also, after probe interrupt reported interrupted/idle, the TUI still refused /fork as task in progress. Several required manual checks remain NOT OBSERVED. No GO is claimed.

Design inputs were read completely, read-only from canonical (gitignored): `D:/Developer/codeterm/references/_reference-app-design/DESIGN.md` and `ADDENDUM-install.md`. No product code was authored.

## Windows setup and exact commands

Machine BLACKSTAR, 2026-10-07. Native binary: `C:/Users/dev/AppData/Local/Programs/OpenAI/Codex/bin/codex.exe`, `codex-cli 0.160.0`. Both server and TUI use this same absolute binary. Owner DEV CLI reported 1.12.4, SHA `54cd23ad35c7570229b19243b1649ca188893723`.

Our PROD Team does not exist in DEV. The Orchestrator explicitly authorized dedicated DEV workspace LaneSpike `e5c0d7ba` and a standalone utility shell with --no-team. No JobsDemo/AiderRun Team was used. All DEV CLI operations used the bench library's dc wrapper. No Domios build/restart occurred.

Initial utility tab `4a62b52c` tested the empty-thread flow and was closed. Subsequent tab `bc042710` tested the three adapted flows sequentially and was closed. Example exact creation command for flow (a):

```bash
source evals/_lib/lib.sh
dc tab new --workspace e5c0d7ba --no-team --title Spike \
  --cwd C:/Users/dev/AppData/Local/Temp \
  --cmd "node D:/Developer/codeterm-worktrees/pl-a0rcq7-scout/evals/codex-remote-spike/launch.mjs --binary C:/Users/dev/AppData/Local/Programs/OpenAI/Codex/bin/codex.exe --port 18471 --dir C:/Users/dev/AppData/Local/Temp/codex-remote-spike-seed-20261007 --flow seed"
```

Flow (b) used `dc tab restart --tab bc042710 --cmd "..."`, with the same launcher command except `--dir C:/Users/dev/AppData/Local/Temp/codex-remote-spike-tui-20261007 --flow tui`.
Recommended injection flow used the same restart with `--dir C:/Users/dev/AppData/Local/Temp/codex-remote-spike-inject-20261007 --flow inject`.
The default `--flow empty` preserves the original failing reproduction.

Launcher spawns:

```text
<ABS_BINARY> app-server --listen ws://127.0.0.1:18471 --ws-auth capability-token --ws-token-file <TEMP>/token
<ABS_BINARY> -c check_for_update_on_startup=false --remote ws://127.0.0.1:18471 --remote-auth-token-env CODETERM_CODEX_RPC_TOKEN resume <ID>
```

Flow tui omits resume. All processes inherit the utility tab's composed environment; only CODETERM_CODEX_RPC_TOKEN is added. CODEX_HOME is never overridden. Token is generated under temp, absent from argv/transcripts, and removed during teardown. No config-writing RPC was called.

An independent probe was attached before PTY input in seed and inject flows. Example:

```powershell
New-Item -ItemType File C:/Users/dev/AppData/Local/Temp/codex-remote-spike-inject-20261007/control.ndjson
node evals/codex-remote-spike/probe.mjs --port 18471 --token-file C:/Users/dev/AppData/Local/Temp/codex-remote-spike-inject-20261007/token --thread 01a11616-07f5-73b1-a880-95c3c4df55cf --control-file C:/Users/dev/AppData/Local/Temp/codex-remote-spike-inject-20261007/control.ndjson --out C:/Users/dev/AppData/Local/Temp/codex-remote-spike-inject-20261007/observer.ndjson
```

The control file is append-only, newline-delimited JSON. fs.watch is event-driven; no polling or sleeps. `--interactive` accepts the same JSON on stdin in an interactive QA shell. The probe uses separate client ID space, classifies server requests before response IDs, captures frames before response matching, and never auto-approves.

## Observed flow results and transcripts

| Flow | Result | Evidence |
|---|---|---|
| Empty mint, immediate resume | FAIL -32600 no rollout found, TUI exits 1; mint connection held open does not help | windows-first-mint.ndjson, windows-held-mint.ndjson, windows-final-mint.ndjson, windows-screen.txt |
| (a) Mint, short probe model turn, then resume | PASS attachment; TUI displays SEED_READY, PTY-entered TUI_TYPED_DELTA, and live SIDECAR_VISIBLE from probe | windows-seed-mint.ndjson, windows-seed-observer.ndjson, windows-seed-screen.txt |
| (b) TUI creates thread, broadcast discovery, probe joins | Immediate resume fails before first turn; explicit resume after first TUI turn succeeds; subsequent TUI deltas and live sidecar turn observed | windows-tui-created-discovery.ndjson, windows-tui-created-observer.ndjson, windows-tui-created-screen.txt |
| Inject developer marker before TUI resume | PASS attachment with no inference spent to materialize; marker hidden; first TUI turn streams to probe subscribed before that turn | windows-inject-mint.ndjson, windows-inject-observer.ndjson, windows-inject-first-screen.txt |

All evidence files are under [evidence/](evidence/). Captured screens are plain terminal text, not screenshots. Input was programmatically entered through Domios's PTY send path; no human typing or resize is claimed. Early unbound TUI input needed a separate submit key after the observed composer contained the payload. A probe harness EOF/output-close race was fixed by ending its transcript only after socket close; that harness error is not a provider failure.

Selected executed results:

- Seed and injection TUI: visible prompt and usable conversation. Sidecar userMessage clientId echoes `spike-sidecar-visible` and `inject-model-next` respectively. Both clients receive streaming deltas and completed turns.
- Model turn: turn/start requested catalogue-listed gpt-6.1-sol; thread/settings/updated reported it and the TUI footer changed to GPT-6.1-Sol. That turn completed with MODEL_NEXT_TURN.
- Approval request 0: TUI displayed command/justification; probe replied decline; both captures received serverRequest/resolved requestId 0. TUI conversation continued with denial. Plain text capture contains residual drawing fragments, so perfectly clean visual dismissal is NOT OBSERVED.
- Approval request 1: provider-declared numbered prompt was inspected; `dc tab respond 3 --tab bc042710` returned ok true. Probe received serverRequest/resolved requestId 1 without responding itself.
- Interrupt: active turn `01a11619-4c77-7cd2-bc1c-e05550edff7c` received turn/interrupt; response {}, turn/completed status interrupted, thread status idle. TUI later repeatedly refused /fork as task in progress. See windows-inject-final-screen.txt; slash switching remains incomplete.
- Tool env: command output prints CODETERM_TAB_ID=bc042710 and nonce 489d5168-7fda-46d5-a2a5-a861617095f9, matching windows-inject-launch.json. Its bash/lib dc identify invocation failed exit 1 with DEV resolution unavailable under readOnly sandbox. This is partial env proof, not a passed codeterm-call oracle.
- Global hooks/list reports both CodeTerm sessionStart/stop entries enabled and trusted; hook/started and hook/completed from ~/.codex/hooks.json are captured. No hook config/trust was changed. Per-server selective disabling is NOT OBSERVED.
- Seed-only history backfill: stable=true, allObservedItemsPresent=true. Full injection-flow history: stable=true, allObservedItemsPresent=false twice. Missing live completed items: `exec-e83a34b8-e168-48b0-8b74-ad9c721e0d1f` and `exec-3dba96f2-d7c5-4cda-b4cf-27334746e8d7` (the declined/canceled approval command rows). The executed environment command row is present with its original ID. This fails full-history item 11.

## Checklist stride per OS

| Item | Windows | macOS |
|---|---|---|
| 1 render/input/resize | Render and PTY input PASS; resize NOT OBSERVED | NOT OBSERVED |
| 2 deterministic identity/slashes/subagent discriminator | Inject binding PASS; /fork refused after remote interrupt; /new /clear /resume and live discriminator NOT OBSERVED | NOT OBSERVED |
| 3 second client/TUI streaming | PASS for seed, TUI-created-after-first-turn, inject; raw deltas captured | NOT OBSERVED |
| 4 sidecar turn live/clientId | PASS | NOT OBSERVED |
| 5 approvals both ways/visible dismissal | Both RPC resolution directions PASS; clean terminal repaint NOT OBSERVED (stale fragments) | NOT OBSERVED |
| 6 interrupt | Protocol status interrupted PASS; TUI busy discrepancy observed afterward | NOT OBSERVED |
| 7 next-turn model | PASS | NOT OBSERVED |
| 8 flag-side findings | Thread sandbox/approval params applied; developer instruction precedence and hook-trust bypass NOT OBSERVED | NOT OBSERVED |
| 9 env in tools/hooks/codeterm call | Tab/nonce in tool output PASS; nested dc call failed sandbox DEV resolution; hook env beyond binding NOT OBSERVED | NOT OBSERVED |
| 10 remote avoids shared daemon | NOT OBSERVED; explicit remote argv alone is insufficient process proof | NOT OBSERVED |
| 11 complete history/stable IDs | FAIL: stable returned IDs, two denied command items missing from full backfill | NOT OBSERVED |
| 12 no orphans | Owned server/TUI PIDs and listener absent; full descendant audit NOT OBSERVED | NOT OBSERVED |
| 13 global hooks/selective disable | Global hooks fired PASS; selective per-server argv disabling NOT OBSERVED | NOT OBSERVED |

Flag-side findings: app-server --help lists -c and feature switches but does not list the TUI's hook-trust/sandbox/approval convenience flags. This help observation does not establish equivalent config behavior. Thread/start and turn/start responses/settings reflect explicit sandbox/approval params. TUI-only `-c check_for_update_on_startup=false` suppresses the update picker without persisting a preference. Developer instruction effectiveness/precedence is untested.

Traps: native .exe avoided the npm .cmd shim (launcher rejects .cmd/.bat); CODEX_HOME was unset and initialize resolved the real ~/.codex; idle unload NOT OBSERVED. startedAt:null IS OBSERVED in turn/start responses, including the next-model turn, and turn/started supplies the real start later. The TUI-created flow queues a sidecar turn while another turn is active. Notification-before-response IS OBSERVED for remoteControl/status/changed around initialize; do not infer turn/start ordering from that. Initial TUI update picker offered 0.160.1; no update or persisted Skip preference was selected. Empty rollout materialization is required; thread/name/set was not used to materialize history.

## Teardown and threads

Every utility tab was closed through dc. Exact owned server/TUI PIDs absent at teardown: final empty 69872/65264, seed 54784/53260, TUI-created 38136/52728, injection 55236/69184. Intermediate update picker 46504/5348 was explicitly stopped after checking native paths. Port 18471 had no listener. Seed/TUI/injection token files absent; initial experimental token removed explicitly. Existing unrelated Codex processes were left alone. Full descendant process audit is not claimed. No owner configuration, hooks, ~/.codex configuration or PROD API was edited; ordinary Codex thread creation used the real shared home.

Minted roots:

```text
01a1160d-18ae-7221-9424-2d4e94967dba
01a1160d-dec1-7040-ba16-6a6d3fa64b66
01a1160e-7fea-70c0-b31a-5bc4d0ffb3be
01a1160f-67a7-77d1-b809-a112d0b3ddeb
01a11612-4537-7801-8b1e-1b583533596c
01a11614-2d5b-7b81-aaec-8e8554af8478
01a11616-07f5-73b1-a880-95c3c4df55cf
```

Additional thread IDs seen only in status notifications: 01a11613-9d95-7a83-a57c-acf74e018ec4, 01a11615-20ef-7ad2-9436-3679d4d2ed25, 01a11617-5864-71b0-af59-d8f26d8c15ba. Their purpose/parentage is NOT OBSERVED; do not rebind to status-only IDs. The probe's --follow is a first-thread discovery experiment, not a production subagent discriminator.

## macOS handoff: Orchestrator forwards to Fulton on eight

NOT EXECUTED. Worker does not contact Fulton directly. Orchestrator forwards these files after real integration or an authorized artifact transfer and supplies actual DEV authorization IDs. macOS uses native PTY, not Windows ConPTY. From the receiving checkout on eight:

```bash
source evals/_lib/lib.sh
binary="$(command -v codex)"
"$binary" --version
spike_dir="$(mktemp -d /tmp/codex-remote-spike.XXXXXX)"
repo="$(git rev-parse --show-toplevel)"
dc tab new --workspace <AUTHORIZED_DEV_WORKSPACE> --team <AUTHORIZED_DEV_TEAM> \
  --title Spike --cwd "$spike_dir" \
  --cmd "node '$repo/evals/codex-remote-spike/launch.mjs' --binary '$binary' --port 18471 --dir '$spike_dir' --flow inject"
```

Replace placeholders before execution; an explicitly authorized standalone shell may use --no-team. command -v must resolve to an absolute native executable; both processes use it. Save returned tab ID; capture `dc tab get-text --tab <TAB>`, launch.json and mint.ndjson. Oracle: minted ID attaches in real TUI with marker hidden, usable conversation, resizing, and first PTY turn reaches a probe subscribed before it. Any picker/socket response alone is insufficient.

Attach the second probe from a QA shell before TUI input:

```bash
node evals/codex-remote-spike/probe.mjs --port 18471 \
  --token-file "$spike_dir/token" --thread "$(cat "$spike_dir/thread")" \
  --interactive --checklist all --out "$spike_dir/observer.ndjson"
```

One JSON command per stdin line (replace IDs from real frames):

```json
{"method":"model/list","params":{}}
{"method":"hooks/list","params":{}}
{"method":"turn/start","params":{"threadId":"REPLACE_ID","clientUserMessageId":"spike-sidecar-1","input":[{"type":"text","text":"Reply with SIDECAR_VISIBLE only. Do not use tools."}],"approvalsReviewer":"user"}}
{"replyTo":0,"result":{"decision":"decline"}}
{"method":"turn/interrupt","params":{"threadId":"REPLACE_ID","turnId":"REPLACE_ACTIVE_TURN_ID"}}
{"action":"backfill"}
{"action":"quit"}
```

Use two harmless explicit escalation requests (print only), inspect TUI approval, decline one from probe and one with dc tab respond using the provider-declared choice. Capture serverRequest/resolved and fresh TUI text both times. Pick next-turn model from model/list, capture settings/model footer. For interrupt, observe turn/started before interrupting and inspect idle TUI afterward. Backfill all cursor pages and compare live completed IDs, including denied commands. Exercise /new /clear /fork /resume and a real subagent discriminator; never bind on a bare status notification. Verify app-server environment inside tool/hook calls without exposing API tokens. Test instruction/sandbox/approval/hook-trust precedence independently via argv/thread params, without config-writing RPC or global edits. Verify selective hook disabling only if a real per-process control exists; otherwise record NOT OBSERVED. Verify remote/shared-daemon process isolation without mutating the shared daemon.

To reproduce both alternative flows, rerun launcher in fresh directories with --flow seed and --flow tui. In tui flow, discovery immediately tries resume and records the expected early failure; after the first TUI turn, attach a fresh probe explicitly to the broadcast ID. No timed retry or newest-thread heuristic is used. Seed flow spends one model turn solely for the test; use a real first task instead in any product design.

After teardown, verify exact saved PIDs and descendants absent, no dedicated listener, close the exact utility tab through dc, and list all created/discovered threads. Send transcripts and item-by-item results through Orchestrator. GO requires every item PASS on both physical machines.

## Validation and pending gates

Executed Node syntax checks for probe.mjs and launch.mjs: exit 0 each. Live empty launcher/TUI: exit 1. Seed, TUI-create and injection flows render/accept input; their long-running launch CLI receipts report timeout while the interactive TUI remains alive, not test success. Approval response CLI: exit 0, ok true. Nested environment-tool command: exit 1. No project build/tests ran.

The later source-only Worker rule reserves gateway verification for canonical after a real merge. Requested canonical command: `.\build.ps1 check-tests`, report raw exit/compiled target counts. No Rust selector is owned by this eval-only change. Focused commands are the two Node syntax checks and exact live launcher/second-client oracles above. Syntax alone never passes the provider gate.
