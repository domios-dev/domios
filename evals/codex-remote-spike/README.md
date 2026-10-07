# Codex remote TUI gate

## Verdict

**Windows: NO-GO. macOS: NO-GO (NOT OBSERVED; external QA leg pending). Lane A stays blocked.**

Windows checklist item 2 fails with Codex 0.160.0: a freshly minted, explicitly non-ephemeral thread cannot be resumed by the TUI. Keeping the minting connection open does not fix it. The TUI exits 1 with RPC -32600, `no rollout found`. This is an executed result, not a claim that all remote TUI configurations fail.

Binding option tested: sidecar `thread/start`, then the real TUI `--remote ... resume <threadId>` using the same absolute binary. A possible alternative is materializing a first turn before attaching the TUI; that is an **untested hypothesis**, not a repair or a GO. No product code was authored.

Design inputs were read completely, read-only from canonical because they are gitignored:
`D:/Developer/codeterm/references/_t3code-design/DESIGN.md` and `ADDENDUM-install.md`.

## Windows execution evidence

Date: 2026-10-07. Machine: BLACKSTAR. Codex: `codex-cli 0.160.0`.
Native binary used for both processes:
`C:/Users/User/AppData/Local/Programs/OpenAI/Codex/bin/codex.exe`.
Owner DEV reported CLI version 1.12.4, SHA `54cd23ad35c7570229b19243b1649ca188893723`.

The Orchestrator authorized dedicated DEV workspace `LaneSpike` (`e5c0d7ba`) and a standalone utility shell because our PROD Team does not exist in DEV. Utility tab: `4a62b52c`. No JobsDemo/AiderRun Team was used. Every DEV CLI operation used the bench library's `dc` wrapper; no build or restart of Domios occurred.

Executed shell creation (the first temporary host preceded the committed launcher):

```bash
source evals/_lib/lib.sh
dc tab new --workspace e5c0d7ba --no-team --title Spike \
  --cwd C:/Users/User/AppData/Local/Temp/codex-remote-spike-3881ece143dc4f7c96553e0e40e4089c \
  --cmd "node C:/Users/User/AppData/Local/Temp/codex-remote-spike-3881ece143dc4f7c96553e0e40e4089c/host.mjs C:/Users/User/AppData/Local/Temp/codex-remote-spike-3881ece143dc4f7c96553e0e40e4089c"
```

Final reproduction with the committed launcher:

```bash
source evals/_lib/lib.sh
dc tab restart --tab 4a62b52c --cmd "node D:/Developer/codeterm-worktrees/pl-a0rcq7-scout/evals/codex-remote-spike/launch.mjs --binary C:/Users/User/AppData/Local/Programs/OpenAI/Codex/bin/codex.exe --port 18471 --dir C:/Users/User/AppData/Local/Temp/codex-remote-spike-final-20261007"
```

The CLI transport command exited 0; its nested `exec.exit_code` was **1**. The terminal transcript, also saved in [windows-screen.txt](evidence/windows-screen.txt):

```text
codex app-server (WebSockets)
  listening on: ws://127.0.0.1:18471
MINTED_THREAD 01a1160f-67a7-77d1-b809-a112d0b3ddeb
Error: Failed to resume session from C:\Users\User\.codex\sessions\2026\10\07\rollout-2026-10-07T14-11-10-01a1160f-67a7-77d1-b809-a112d0b3ddeb.jsonl: thread/resume failed during TUI bootstrap: thread/resume failed: no rollout found for thread id 01a1160f-67a7-77d1-b809-a112d0b3ddeb (code -32600)
TUI_EXIT 1
SERVER_EXIT null
```

Wire captures:

- [windows-first-mint.ndjson](evidence/windows-first-mint.ndjson): initialize, thread/start `{ephemeral:false}`, response with ID, immediate same-client resume error -32600.
- [windows-held-mint.ndjson](evidence/windows-held-mint.ndjson): minting client held open; TUI resume still fails.
- [windows-final-mint.ndjson](evidence/windows-final-mint.ndjson): reproduction with the committed launcher. `thread/started` is broadcast after the mint response. `parentThreadId:null`, `threadSource:null`, `source:"vscode"`, no turns.
- [windows-launch.json](evidence/windows-launch.json): composed utility-tab identity and nonce inherited by app-server and TUI; CODEX_HOME was not overridden. This proves process launch environment only, not tool/hook inheritance.

## Checklist

`NOT OBSERVED` never counts as PASS. The failure at item 2 prevented downstream live checks; no unrelated existing Codex thread was used to bypass it.

| Item | Windows | macOS |
|---|---|---|
| 1: render, input, resize | NOT OBSERVED: update picker rendered in ConPTY on intermediate attempt; final TUI bootstrap failed before conversation/input/resize | NOT OBSERVED |
| 2: deterministic mint/resume, slash switches, subagents | FAIL: minted empty thread cannot resume; slash switches and live subagent discriminator NOT OBSERVED | NOT OBSERVED |
| 3: second client receives TUI-typed streaming | NOT OBSERVED | NOT OBSERVED |
| 4: probe turn visible in TUI, clientId echo | NOT OBSERVED | NOT OBSERVED |
| 5: approval dismissal in both directions | NOT OBSERVED | NOT OBSERVED |
| 6: interrupt ends with interrupted | NOT OBSERVED | NOT OBSERVED |
| 7: next-turn model change | NOT OBSERVED | NOT OBSERVED |
| 8: developer instructions, sandbox/approval, hook-trust flag side | NOT OBSERVED end-to-end; mint response reflects thread params readOnly/on-request/user | NOT OBSERVED |
| 9: app-server env in tools/hooks and agent codeterm call | NOT OBSERVED | NOT OBSERVED |
| 10: remote excludes shared daemon | NOT OBSERVED: explicit remote argv alone is insufficient proof | NOT OBSERVED |
| 11: paginated backfill, stable ids | NOT OBSERVED | NOT OBSERVED |
| 12: no orphans | Server/TUI PIDs absent and dedicated listener absent; full descendant audit NOT OBSERVED | NOT OBSERVED |
| 13: global hooks fire, argv can disable owned entries | NOT OBSERVED | NOT OBSERVED |

Flag-side findings: `app-server --help` accepts `-c` and feature switches, but does not list the TUI's `--dangerously-bypass-hook-trust` or sandbox/approval convenience flags. This help observation does not establish equivalent config behavior. Thread/start response reflected the explicit sandbox/approval params. Developer instruction effectiveness was not tested. TUI-only `-c check_for_update_on_startup=false` suppressed the intermediate update picker; this does not establish instruction forwarding.

Traps recorded:

| Trap | Observation |
|---|---|
| npm .cmd shim | Not encountered: native absolute .exe used; launcher rejects .cmd/.bat rather than inventing shell escaping |
| CODEX_HOME | Unset in tested launch; initialize resolved `C:\Users\User\.codex`; never overridden |
| idle unload | NOT OBSERVED; failure was immediate, before idle unload could explain it |
| startedAt:null | NOT OBSERVED; no turn/start executed |
| notification-before-response | NOT OBSERVED for turn/start; initialized broadcast interleaved with request traffic |
| empty-thread persistence | OBSERVED: non-ephemeral start returns a rollout path that resume cannot load, including with mint connection held open |
| update picker | OBSERVED: default TUI launch offered 0.160.1; no update or persisted Skip preference was selected |

Created thread IDs (including unsuccessful attempts):

```text
01a1160d-18ae-7221-9424-2d4e94967dba
01a1160d-dec1-7040-ba16-6a6d3fa64b66
01a1160e-7fea-70c0-b31a-5bc4d0ffb3be
01a1160f-67a7-77d1-b809-a112d0b3ddeb
```

Teardown: failed TUI caused the launcher to terminate its owned server and mint client. Final server PID 69872 and TUI PID 65264 were absent; preceding server PID 53764 and TUI PID 67784 were absent. Intermediate update-picker TUI PID 46504 and server PID 5348 were explicitly stopped after verifying their native executable paths. No listener remained on 18471. Final launcher's token file was deleted. `dc tab close --tab 4a62b52c` returned `ok true`. Existing unrelated Codex processes were left alone; global `tasklist | grep codex` emptiness would be an invalid oracle on this agent machine. Temporary evidence files from initial experimental hosts remain under the named temp directory; their token file was removed and no auth token is committed.

No config-writing RPC was called. No owner config, hooks or Codex configuration file was edited. Normal thread creation used the real shared Codex home as required. No PROD API operation or product code change was performed.

## macOS handoff: Orchestrator forwards to Fulton on eight

**NOT EXECUTED.** The Worker does not contact Fulton directly. Orchestrator supplies these files after a real merge (or an explicitly authorized artifact transfer), and authorizes the DEV workspace/Team on eight. Do not target PROD or edit global config/hooks. macOS uses a native PTY, not Windows ConPTY.

From canonical on eight, choose a fresh temp directory and the native Codex binary. Use the same absolute binary for both sides (the launcher enforces this):

```bash
source evals/_lib/lib.sh
binary="$(command -v codex)"
"$binary" --version
spike_dir="$(mktemp -d /tmp/codex-remote-spike.XXXXXX)"
repo="$(git rev-parse --show-toplevel)"
dc tab new --workspace <AUTHORIZED_DEV_WORKSPACE> --team <AUTHORIZED_DEV_TEAM> \
  --title Spike --cwd "$spike_dir" \
  --cmd "node '$repo/evals/codex-remote-spike/launch.mjs' --binary '$binary' --port 18471 --dir '$spike_dir'"
```

Replace the two authorization placeholders with actual IDs before execution. An explicitly authorized standalone shell can use `--no-team` instead. Save the returned tab ID, `dc tab get-text --tab <TAB>`, and mint.ndjson. Oracle: a minted ID followed by a usable remote conversation is required; `TUI_EXIT 1`/`no rollout found` is FAIL item 2. Do not claim GO from a visible picker or listening socket. If bootstrap fails, record its exact transcript and NO-GO, tear down and report.

If bootstrap succeeds, keep the launcher and minter alive. Attach a separate client in a QA shell:

```bash
node evals/codex-remote-spike/probe.mjs --port 18471 \
  --token-file "$spike_dir/token" --thread "$(cat "$spike_dir/thread")" \
  --interactive --checklist all --out "$spike_dir/observer.ndjson"
```

The probe accepts one JSON object per stdin line, uses distinct client request IDs, captures all received frames before matching responses, and never auto-approves. Commands:

```json
{"method":"model/list","params":{}}
{"method":"turn/start","params":{"threadId":"REPLACE_ID","clientUserMessageId":"spike-sidecar-1","input":[{"type":"text","text":"Reply with SIDECAR_VISIBLE only."}],"approvalsReviewer":"user"}}
{"replyTo":0,"result":{"decision":"decline"}}
{"method":"turn/interrupt","params":{"threadId":"REPLACE_ID","turnId":"REPLACE_ACTIVE_TURN_ID"}}
{"action":"backfill"}
{"action":"quit"}
```

Use actual approval/request/turn IDs from frames. For a model switch, pick a second supported model from model/list and include `model` in the next turn/start; capture settings and the completed turn. Backfill fetches all cursor pages twice and records stable IDs plus inclusion of observed completed items; it does not prove unobserved history automatically.

Record visible TUI input and deltas in observer.ndjson, sidecar turn rendering, approval rendering and dismissal in each direction, slash commands `/new`, `/clear`, `/fork`, `/resume`, and a live subagent's source/parent linkage. Record actual resizing and resulting TUI rendering. Exercise developer instructions independently on TUI argv, app-server `-c`, and thread params with distinct response markers. Exercise sandbox/approval and hook trust independently without modifying global files. Capture hooks/list and hook/started/completed, then verify whether a per-process config override can disable only owned entries; never mutate hook trust/config with RPC. Have a safe agent tool command invoke `codeterm identify` under the DEV-composed environment and print tab ID/nonce, never API tokens. Verify remote behavior against shared-daemon process evidence without killing or changing that daemon.

The summary intentionally prints NOT OBSERVED for items 2–13 until a reviewer pairs protocol frames with these independent visible/environment/lifecycle oracles. It cannot automatically certify the full manual checklist.

After exit, verify the saved server/TUI PIDs and descendant processes are absent, port 18471 has no listener, close the exact utility tab with dc, and list every minted/switched/subagent thread ID. Send transcripts and per-item verdicts back through the Orchestrator. GO requires every item PASS on both physical machines.

## Validation and pending gates

Executed: `node --check evals/codex-remote-spike/probe.mjs` and `node --check evals/codex-remote-spike/launch.mjs`, exit 0 each. Live committed launcher: TUI/launcher exit 1, zero conversation tests passed. No project tests ran.

The later source-only Worker rule reserves gateway checks for canonical after a real merge. Requested canonical command: `.\build.ps1 check-tests --wait-build` (report raw exit and compiled targets). There is no Rust selector owned by this eval-only change. Focused verification is the two Node syntax commands and the exact live reproduction above; a successful syntax check is not a passed provider lane.
