# Codex app-server protocol coverage

Audit of every method and notification in `src-tauri/src/service/codex_rpc/schema/codex_app_server_protocol.v2.schemas.json` (codex-cli 0.160.0) plus the standalone `ServerRequest`/`ClientNotification` schemas: 167 client requests, 83 server notifications, 11 server requests, 1 client notification (262 entries). The Chat-only runtime lives in `plugins/provider-codex` (`runtime.ts` plans requests, `runtimeReduce.ts` reduces frames, `runtimeTurnStatus.ts` maps statuses); core sees only the provider-neutral `agentRuntime` capability and `AgentEvent`s. Reference: T3 `apps/server/src/orchestration-v2/Adapters/CodexAdapterV2.ts`.

Decisions: **USED** (already consumed), **ADOPTED** (consumed as of this audit), **ADOPT → slice** (proposed follow-up below), **COVERED** (owned by another task), **SKIP** (with the reason). The T3 column says where T3 references the method: `adapter` (CodexAdapterV2), `provider probe` (CodexProvider/Driver/ThreadRevert), `event logger only`, or `—` (unused).

## Status mapping (shared agent state)

Codex statuses become one `turnState` event with an optional closed `detail`; the host maps it onto the tab's `AgentLiveState` (sidebar dot, tab status, composer strip) and the UI owns the words.

| Codex signal | turnState | Live state (state / subtype / phase) | Shown as |
|---|---|---|---|
| `thread/status/changed` idle | idle | Idle | Idle |
| `thread/status/changed` active, no flags | working (+ compacting while a compaction item is open) | Working | Working |
| active + `waitingOnApproval`; any `*/requestApproval` request | needs_input / awaitingApproval | Question / permission | Permission needed |
| active + `waitingOnUserInput`; `item/tool/requestUserInput`, `mcpServer/elicitation/request` | needs_input / awaitingInput | Question | Waiting for input (or the pending prompt) |
| `thread/status/changed` systemError | needs_input / systemError | Question / error | Needs attention |
| `thread/status/changed` notLoaded | none | unchanged | runtime lifecycle (`threadUnloaded`) owns it |
| `turn/started` | working | Working | Working |
| `item/started` contextCompaction | working / compacting | Working / phase compacting | Compacting |
| `item/completed` contextCompaction | working (same turn) + `contextCompacted` | Working | Working; host restores the developer contract |
| `turn/completed` completed | idle | Idle | Idle |
| `turn/completed` interrupted | idle / interrupted | Idle / phase interrupted | Interrupted |
| `turn/completed` failed | idle / failed (+ error row) | Idle / phase failed | Turn failed |

A provider `statusLine` (bounded to 160 characters by the host) takes precedence over these labels; the abandoned-turn lane (task_4i7cct) emits it.

## Compaction facts (live capture, codex-cli 0.160.0)

`plugins/provider-codex/tests/fixtures/runtime/captured/compaction.json` records one short turn followed by `thread/compact/start`. Codex runs compaction as its own turn: `thread/status/changed` active → `turn/started` → `item/started{contextCompaction}` → `thread/tokenUsage/updated` → `item/completed{contextCompaction}` → `thread/status/changed` idle → `turn/completed`. It never sends the deprecated `thread/compacted`, and it emits no status flag for compaction, so the item is the only signal (T3 reaches the same conclusion).

Codex drops client-injected developer items during compaction (T3 notes the same). Domios's contract is safe: `thread/start`/`thread/resume` pass it as `developerInstructions`, and after every completed contextCompaction the host (`service/agent_runtime/subscriber/compaction.rs`) re-injects the tab's system prompt through the `restoreContext` control (`thread/inject_items`, developer role), deduplicated per item and epoch. It holds sends until the restore succeeds and degrades visibly when it fails. The `additionalContext` list is always empty today (see F14).

## Client→server requests (167)

| Method | What it does | Domios uses | Decision | T3 |
|---|---|---|---|---|
| `initialize` | Handshake; declares experimentalApi | yes | USED: Agent providers (runtime initialize) | adapter |
| `server/diagnostics` | Process-local diagnostics | no | SKIP: health and logs cover it | — |
| `userVerification/status` | Account user verification | no | SKIP: account management stays in the provider's own UI | — |
| `userVerification/enroll` | Account user verification | no | SKIP: account management stays in the provider's own UI | — |
| `userVerification/delete` | Account user verification | no | SKIP: account management stays in the provider's own UI | — |
| `userVerification/verify` | Account user verification | no | SKIP: account management stays in the provider's own UI | — |
| `userVerification/cancel` | Account user verification | no | SKIP: account management stays in the provider's own UI | — |
| `thread/start` | Open a thread with cwd, model, policy and developerInstructions | yes | USED: `openThread`; Domios contract goes in developerInstructions | adapter |
| `thread/resume` | Reload a persisted thread | yes | USED: `resumeThread` (instructions re-sent) | adapter |
| `thread/fork` | Fork a thread at a turn boundary | no | ADOPT → F1 thread fork and revert | adapter: fork at a resolved turn boundary |
| `thread/archive` | Archive a thread | no | SKIP: Domios never hides provider history; unarchive is used for resume | — |
| `thread/delete` | Delete a thread | no | SKIP: unsafe, destroys provider history | — |
| `thread/unsubscribe` | Drop a connection's subscription | no | SKIP: one process per tab, so close ends the process (T3 needs it for a shared app-server) | adapter: unload thread on shared app-server |
| `thread/increment_elicitation` | Pause timeout accounting for external helpers | no | SKIP: no external elicitation helper | — |
| `thread/decrement_elicitation` | Pause timeout accounting for external helpers | no | SKIP: no external elicitation helper | — |
| `thread/name/set` | Set thread name | no | ADOPT → F2 provider session name (session title only; tab titles never change) | — |
| `thread/goal/set` | Set/clear the thread goal | no | ADOPT → F3 goal control | adapter: pauses/sets goals around its own goal turns |
| `thread/goal/get` | Read thread goal | yes | USED: history goal row (sessionIntent) | adapter |
| `thread/goal/clear` | Set/clear the thread goal | no | ADOPT → F3 goal control | adapter: /goal clear |
| `thread/queue/add` | Provider-side queued user messages | no | SKIP: message-delivery invariant writes every post-ready send immediately and steers a busy turn; a second queue would hold input the ledger reports as written. Held sends cover pre-ready only | — |
| `thread/queue/list` | Provider-side queued user messages | no | SKIP: message-delivery invariant writes every post-ready send immediately and steers a busy turn; a second queue would hold input the ledger reports as written. Held sends cover pre-ready only | — |
| `thread/queue/update` | Provider-side queued user messages | no | SKIP: message-delivery invariant writes every post-ready send immediately and steers a busy turn; a second queue would hold input the ledger reports as written. Held sends cover pre-ready only | — |
| `thread/queue/delete` | Provider-side queued user messages | no | SKIP: message-delivery invariant writes every post-ready send immediately and steers a busy turn; a second queue would hold input the ledger reports as written. Held sends cover pre-ready only | — |
| `thread/queue/reorder` | Provider-side queued user messages | no | SKIP: message-delivery invariant writes every post-ready send immediately and steers a busy turn; a second queue would hold input the ledger reports as written. Held sends cover pre-ready only | — |
| `thread/queue/start` | Provider-side queued user messages | no | SKIP: message-delivery invariant writes every post-ready send immediately and steers a busy turn; a second queue would hold input the ledger reports as written. Held sends cover pre-ready only | — |
| `thread/metadata/update` | Codex app thread organization | no | SKIP: workspaces and tabs own organization | — |
| `thread/attachment/add` | Codex app thread organization | no | SKIP: workspaces and tabs own organization | — |
| `thread/attachment/list` | Codex app thread organization | no | SKIP: workspaces and tabs own organization | — |
| `thread/attachment/remove` | Codex app thread organization | no | SKIP: workspaces and tabs own organization | — |
| `thread/section/move` | Codex app thread organization | no | SKIP: workspaces and tabs own organization | — |
| `thread/settings/update` | Change model/effort/policy for next turns | yes | USED: NextTurn model, effort, mode | — |
| `thread/memoryMode/set` | Codex native memory | no | SKIP: durable memory lives only in Domios memory, never provider side stores | — |
| `memory/status` | Codex native memory | no | SKIP: durable memory lives only in Domios memory, never provider side stores | — |
| `memory/reset` | Codex native memory | no | SKIP: durable memory lives only in Domios memory, never provider side stores | — |
| `rollout/compress` | Compress cold rollouts | no | SKIP: provider storage maintenance | — |
| `thread/unarchive` | Unarchive before resume | yes | USED: archived-resume recovery | adapter |
| `thread/compact/start` | Manual compaction (runs as its own turn) | yes | USED: `compact` control | adapter: manual compact as a pending root turn |
| `thread/shellCommand` | Run a user shell command in the thread | no | SKIP: shell tabs cover it; revisit only with a composer `!` capability | — |
| `thread/approveGuardianDeniedAction` | Guardian auto-approval review | no | ADOPT → F12 auto-review visibility | — |
| `thread/backgroundTerminals/clean` | Background terminals of a thread | no | COVERED: task_jo0kfs children | — |
| `thread/backgroundTerminals/list` | Background terminals of a thread | no | COVERED: task_jo0kfs children | adapter: list then terminate by processId |
| `thread/backgroundTerminals/terminate` | Background terminals of a thread | no | COVERED: task_jo0kfs children | adapter: terminate by processId |
| `thread/revert` | Revert thread to a turn | no | ADOPT → F1 thread fork and revert | provider probe: CodexThreadRevert via thread/read + turns/list |
| `thread/list` | List and search threads | no | SKIP: `sessions` searches every provider store uniformly; a provider API would be Codex-only | — |
| `project/list` | Codex app projects | no | SKIP: Domios workspaces are the project model | — |
| `project/read` | Codex app projects | no | SKIP: Domios workspaces are the project model | — |
| `project/create` | Codex app projects | no | SKIP: Domios workspaces are the project model | — |
| `project/import` | Codex app projects | no | SKIP: Domios workspaces are the project model | — |
| `project/update` | Codex app projects | no | SKIP: Domios workspaces are the project model | — |
| `project/move` | Codex app projects | no | SKIP: Domios workspaces are the project model | — |
| `project/delete` | Codex app projects | no | SKIP: Domios workspaces are the project model | — |
| `threadSection/list` | Codex app sections | no | SKIP: irrelevant | — |
| `threadSection/create` | Codex app sections | no | SKIP: irrelevant | — |
| `threadSection/update` | Codex app sections | no | SKIP: irrelevant | — |
| `threadSection/delete` | Codex app sections | no | SKIP: irrelevant | — |
| `thread/search` | List and search threads | no | SKIP: `sessions` searches every provider store uniformly; a provider API would be Codex-only | — |
| `thread/searchOccurrences` | List and search threads | no | SKIP: `sessions` searches every provider store uniformly; a provider API would be Codex-only | — |
| `thread/loaded/list` | List and search threads | no | SKIP: `sessions` searches every provider store uniformly; a provider API would be Codex-only | — |
| `thread/read` | Snapshot a thread (optionally with turns) | yes | USED: resume identity verify; history fallback on -32601 | adapter |
| `thread/turns/list` | Paginated history | yes | USED: `readHistory` backfill | provider probe |
| `thread/items/list` | Paginated history | yes | USED: `readHistory` backfill | — |
| `thread/inject_items` | Append raw items without a turn | yes | USED: binding marker on open; `restoreContext` after every completed contextCompaction (host `subscriber/compaction.rs`) | adapter: restores additionalContext after each compaction; also injects history |
| `skills/list` | List provider skills | no | ADOPT → F9 provider skills listing | provider probe: provider skills probe |
| `skills/extraRoots/set` | Write skill roots/config | no | SKIP: skills register through Domios memory, never provider skill directories | — |
| `hooks/list` | List configured hooks | no | ADOPT → F8 provider hook activity | — |
| `marketplace/add` | Codex plugin/app ecosystem | no | SKIP: Domios plugins and channels own extension; installs would mutate the owner's global Codex | — |
| `marketplace/remove` | Codex plugin/app ecosystem | no | SKIP: Domios plugins and channels own extension; installs would mutate the owner's global Codex | — |
| `marketplace/upgrade` | Codex plugin/app ecosystem | no | SKIP: Domios plugins and channels own extension; installs would mutate the owner's global Codex | — |
| `plugin/list` | Codex plugin/app ecosystem | no | SKIP: Domios plugins and channels own extension; installs would mutate the owner's global Codex | — |
| `plugin/search` | Codex plugin/app ecosystem | no | SKIP: Domios plugins and channels own extension; installs would mutate the owner's global Codex | — |
| `plugin/installed` | Codex plugin/app ecosystem | no | SKIP: Domios plugins and channels own extension; installs would mutate the owner's global Codex | — |
| `plugin/reconcile` | Codex plugin/app ecosystem | no | SKIP: Domios plugins and channels own extension; installs would mutate the owner's global Codex | — |
| `plugin/read` | Codex plugin/app ecosystem | no | SKIP: Domios plugins and channels own extension; installs would mutate the owner's global Codex | — |
| `plugin/skill/read` | Codex plugin/app ecosystem | no | SKIP: Domios plugins and channels own extension; installs would mutate the owner's global Codex | — |
| `plugin/share/save` | Codex plugin/app ecosystem | no | SKIP: Domios plugins and channels own extension; installs would mutate the owner's global Codex | — |
| `plugin/share/updateTargets` | Codex plugin/app ecosystem | no | SKIP: Domios plugins and channels own extension; installs would mutate the owner's global Codex | — |
| `plugin/share/list` | Codex plugin/app ecosystem | no | SKIP: Domios plugins and channels own extension; installs would mutate the owner's global Codex | — |
| `plugin/share/checkout` | Codex plugin/app ecosystem | no | SKIP: Domios plugins and channels own extension; installs would mutate the owner's global Codex | — |
| `plugin/share/delete` | Codex plugin/app ecosystem | no | SKIP: Domios plugins and channels own extension; installs would mutate the owner's global Codex | — |
| `app/read` | Codex plugin/app ecosystem | no | SKIP: Domios plugins and channels own extension; installs would mutate the owner's global Codex | — |
| `app/list` | Codex plugin/app ecosystem | no | SKIP: Domios plugins and channels own extension; installs would mutate the owner's global Codex | — |
| `app/installed` | Codex plugin/app ecosystem | no | SKIP: Domios plugins and channels own extension; installs would mutate the owner's global Codex | — |
| `fs/readFile` | Filesystem through the server | no | SKIP: unsafe and redundant; Domios reads files through its tab-scoped reader | — |
| `fs/writeFile` | Filesystem through the server | no | SKIP: unsafe and redundant; Domios reads files through its tab-scoped reader | — |
| `fs/createDirectory` | Filesystem through the server | no | SKIP: unsafe and redundant; Domios reads files through its tab-scoped reader | — |
| `fs/getMetadata` | Filesystem through the server | no | SKIP: unsafe and redundant; Domios reads files through its tab-scoped reader | — |
| `fs/readDirectory` | Filesystem through the server | no | SKIP: unsafe and redundant; Domios reads files through its tab-scoped reader | — |
| `fs/remove` | Filesystem through the server | no | SKIP: unsafe and redundant; Domios reads files through its tab-scoped reader | — |
| `fs/copy` | Filesystem through the server | no | SKIP: unsafe and redundant; Domios reads files through its tab-scoped reader | — |
| `fs/watch` | Filesystem through the server | no | SKIP: unsafe and redundant; Domios reads files through its tab-scoped reader | — |
| `fs/unwatch` | Filesystem through the server | no | SKIP: unsafe and redundant; Domios reads files through its tab-scoped reader | — |
| `skills/config/write` | Write skill roots/config | no | SKIP: skills register through Domios memory, never provider skill directories | — |
| `plugin/install` | Codex plugin/app ecosystem | no | SKIP: Domios plugins and channels own extension; installs would mutate the owner's global Codex | — |
| `plugin/uninstall` | Codex plugin/app ecosystem | no | SKIP: Domios plugins and channels own extension; installs would mutate the owner's global Codex | — |
| `turn/start` | Start, steer and interrupt turns | yes | USED: message delivery (steer while busy, exact-refusal fallback to start) | adapter |
| `turn/settings/update` | Change settings of the running turn | no | SKIP: NextTurn contract applies changes to the next turn | — |
| `turn/steer` | Start, steer and interrupt turns | yes | USED: message delivery (steer while busy, exact-refusal fallback to start) | adapter |
| `turn/interrupt` | Start, steer and interrupt turns | yes | USED: message delivery (steer while busy, exact-refusal fallback to start) | adapter |
| `thread/realtime/start` | Realtime voice session | no | SKIP: voice enters through the transcription contract; realtime voice is roadmap | — |
| `thread/realtime/appendAudio` | Realtime voice session | no | SKIP: voice enters through the transcription contract; realtime voice is roadmap | — |
| `thread/realtime/appendText` | Realtime voice session | no | SKIP: voice enters through the transcription contract; realtime voice is roadmap | — |
| `thread/realtime/appendSpeech` | Realtime voice session | no | SKIP: voice enters through the transcription contract; realtime voice is roadmap | — |
| `thread/realtime/stop` | Realtime voice session | no | SKIP: voice enters through the transcription contract; realtime voice is roadmap | — |
| `thread/timeline/list` | Timeline view of a thread | no | SKIP: turns/items pagination covers history | — |
| `thread/realtime/listVoices` | Realtime voice session | no | SKIP: voice enters through the transcription contract; realtime voice is roadmap | — |
| `review/start` | Start a review turn | no | ADOPT → F4 review mode (entered/exitedReviewMode items already render) | — |
| `model/list` | Live model catalogue | no | ADOPT → F5 live pickers (agent models contract: a live catalogue defines launchable models) | provider probe: provider model probe |
| `account/gatewayOAuth/read` | Gateway OAuth and Bedrock auth | no | SKIP: sign-in capability declares browser, device-code and API key; enterprise auth stays in Codex | — |
| `account/gatewayOAuth/login` | Gateway OAuth and Bedrock auth | no | SKIP: sign-in capability declares browser, device-code and API key; enterprise auth stays in Codex | — |
| `account/gatewayOAuth/cancel` | Gateway OAuth and Bedrock auth | no | SKIP: sign-in capability declares browser, device-code and API key; enterprise auth stays in Codex | — |
| `modelProvider/capabilities/read` | Mode, permission-profile and provider capability presets | no | ADOPT → F5 live pickers | — |
| `experimentalFeature/list` | List experimental features | no | ADOPT read-only inside F10 effective config read | — |
| `permissionProfile/list` | Mode, permission-profile and provider capability presets | no | ADOPT → F5 live pickers | — |
| `experimentalFeature/enablement/set` | Toggle experimental feature | no | SKIP: global mutation | — |
| `remoteControl/enable` | Codex remote control pairing | no | SKIP: the mesh and web UI are Domios's remote story | — |
| `remoteControl/disable` | Codex remote control pairing | no | SKIP: the mesh and web UI are Domios's remote story | — |
| `remoteControl/status/read` | Codex remote control pairing | no | SKIP: the mesh and web UI are Domios's remote story | — |
| `remoteControl/pairing/start` | Codex remote control pairing | no | SKIP: the mesh and web UI are Domios's remote story | — |
| `remoteControl/pairing/status` | Codex remote control pairing | no | SKIP: the mesh and web UI are Domios's remote story | — |
| `remoteControl/client/list` | Codex remote control pairing | no | SKIP: the mesh and web UI are Domios's remote story | — |
| `remoteControl/client/revoke` | Codex remote control pairing | no | SKIP: the mesh and web UI are Domios's remote story | — |
| `collaborationMode/list` | Mode, permission-profile and provider capability presets | no | ADOPT → F5 live pickers | — |
| `mock/experimentalMethod` | Test-only | no | SKIP: test-only | — |
| `environment/add` | Remote execution environments | no | SKIP: Domios runs agents on mesh peers | — |
| `environment/info` | Remote execution environments | no | SKIP: Domios runs agents on mesh peers | — |
| `environment/status` | Remote execution environments | no | SKIP: Domios runs agents on mesh peers | — |
| `mcpServer/oauth/login` | MCP server OAuth | no | ADOPT later inside F7 MCP server status: one-click re-auth | — |
| `config/mcpServer/reload` | Reload MCP config | no | ADOPT inside F7 MCP server status (explicit owner action) | — |
| `mcpServerStatus/list` | MCP server status and startup | no | ADOPT → F7 MCP server status | adapter: MCP apps tool listing (toolsAndAuthOnly) |
| `mcpServer/resource/read` | Direct MCP calls, resources and event streams (MCP apps) | no | SKIP: Domios renders no MCP apps; revisit after F7 MCP server status | adapter: MCP apps resources |
| `mcpServer/event/stream/start` | Direct MCP calls, resources and event streams (MCP apps) | no | SKIP: Domios renders no MCP apps; revisit after F7 MCP server status | — |
| `mcpServer/event/stream/stop` | Direct MCP calls, resources and event streams (MCP apps) | no | SKIP: Domios renders no MCP apps; revisit after F7 MCP server status | — |
| `mcpServer/tool/call` | Direct MCP calls, resources and event streams (MCP apps) | no | SKIP: Domios renders no MCP apps; revisit after F7 MCP server status | adapter: MCP apps tool call |
| `windowsSandbox/setupStart` | Windows sandbox setup and warnings | no | SKIP now; warnings join F11 provider notices and safety buffering, setup stays in Codex | — |
| `windowsSandbox/readiness` | Windows sandbox setup and warnings | no | SKIP now; warnings join F11 provider notices and safety buffering, setup stays in Codex | — |
| `account/login/start` | Browser, device-code and API-key sign-in | yes | USED: runtime sign-in (`login` control) | — |
| `account/bedrock/discover` | Gateway OAuth and Bedrock auth | no | SKIP: sign-in capability declares browser, device-code and API key; enterprise auth stays in Codex | — |
| `account/bedrock/setup` | Gateway OAuth and Bedrock auth | no | SKIP: sign-in capability declares browser, device-code and API key; enterprise auth stays in Codex | — |
| `account/login/cancel` | Browser, device-code and API-key sign-in | yes | USED: runtime sign-in (`login` control) | — |
| `account/logout` | Sign out | no | ADOPT → F6 account logout and usage read (`agent logout`) | — |
| `account/rateLimits/read` | Read rate limits on demand | no | COVERED: task_aibmwu | provider probe: provider usage probe |
| `account/rateLimitResetCredit/consume` | Spend a rate-limit reset credit | no | SKIP: unsafe, spends account credit | provider probe |
| `account/usage/read` | Account token usage summary | no | COVERED: task_aibmwu (usage); read path in F6 account logout and usage read only if aibmwu needs it | — |
| `account/workspaceMessages/read` | Workspace messages, credit nudge email | no | SKIP: irrelevant; sends email | — |
| `account/sendAddCreditsNudgeEmail` | Workspace messages, credit nudge email | no | SKIP: irrelevant; sends email | — |
| `feedback/upload` | Upload a provider bug report with logs | no | ADOPT (optional) → F13 provider feedback upload; owner-confirmed, never automatic | adapter: bug report with logs |
| `command/exec` | Standalone exec/process outside a turn | no | SKIP: unsafe second execution path; PTY tabs own shells | — |
| `command/exec/write` | Standalone exec/process outside a turn | no | SKIP: unsafe second execution path; PTY tabs own shells | — |
| `command/exec/terminate` | Standalone exec/process outside a turn | no | SKIP: unsafe second execution path; PTY tabs own shells | — |
| `command/exec/resize` | Standalone exec/process outside a turn | no | SKIP: unsafe second execution path; PTY tabs own shells | — |
| `process/spawn` | Standalone exec/process outside a turn | no | SKIP: unsafe second execution path; PTY tabs own shells | — |
| `process/writeStdin` | Standalone exec/process outside a turn | no | SKIP: unsafe second execution path; PTY tabs own shells | — |
| `process/kill` | Standalone exec/process outside a turn | no | SKIP: unsafe second execution path; PTY tabs own shells | — |
| `process/resizePty` | Standalone exec/process outside a turn | no | SKIP: unsafe second execution path; PTY tabs own shells | — |
| `config/read` | Effective config and requirements | no | ADOPT → F10 effective config read (read-only diagnostics) | — |
| `externalAgentConfig/detect` | Import other agents' config | no | SKIP: irrelevant; would rewrite global provider config | — |
| `externalAgentConfig/import` | Import other agents' config | no | SKIP: irrelevant; would rewrite global provider config | — |
| `externalAgentConfig/import/recordHistory` | Import other agents' config | no | SKIP: irrelevant; would rewrite global provider config | — |
| `externalAgentConfig/import/readHistories` | Import other agents' config | no | SKIP: irrelevant; would rewrite global provider config | — |
| `config/value/write` | Write Codex config | no | SKIP: global config shared with every Codex instance (same danger as hook files) | — |
| `config/batchWrite` | Write Codex config | no | SKIP: global config shared with every Codex instance (same danger as hook files) | — |
| `configRequirements/read` | Effective config and requirements | no | ADOPT → F10 effective config read (read-only diagnostics) | — |
| `account/read` | Read sign-in state | yes | USED: initialize + `auth` control → Chat sign-in card | provider probe |
| `fuzzyFileSearch` | Fuzzy file search for mentions | no | SKIP: Domios project files and composer references cover mentions provider-neutrally | — |
| `fuzzyFileSearch/sessionStart` | Fuzzy file search for mentions | no | SKIP: Domios project files and composer references cover mentions provider-neutrally | — |
| `fuzzyFileSearch/sessionUpdate` | Fuzzy file search for mentions | no | SKIP: Domios project files and composer references cover mentions provider-neutrally | — |
| `fuzzyFileSearch/sessionStop` | Fuzzy file search for mentions | no | SKIP: Domios project files and composer references cover mentions provider-neutrally | — |

## Server→client notifications (83)

| Method | What it does | Domios uses | Decision | T3 |
|---|---|---|---|---|
| `error` | Turn error, possibly retrying | yes | USED: system row (provider_retry or api_error) | adapter |
| `thread/started` | Thread announced (incl. sub-agents) | yes | USED: sub-agent parent registry | — |
| `thread/status/changed` | Thread status: notLoaded, idle, systemError, active{waitingOnApproval|waitingOnUserInput} | yes | ADOPTED here: full mapping to turnState detail (see Status mapping) | — |
| `thread/archived` | Archive state changed | no | SKIP: no archive surface | — |
| `thread/deleted` | Delete a thread | no | SKIP: unsafe, destroys provider history | — |
| `thread/unarchived` | Archive state changed | no | SKIP: no archive surface | — |
| `thread/closed` | Thread closed on the server | no | SKIP: process-per-tab runtime; process exit and threadUnloaded own this | — |
| `thread/reverted` | Revert completed | no | ADOPT → F1 thread fork and revert | — |
| `skills/changed` | Skills changed | no | ADOPT → F9 provider skills listing | — |
| `thread/name/updated` | Thread name changed | no | ADOPT → F2 provider session name | — |
| `thread/attachment/updated` | Codex app thread organization | no | SKIP: workspaces and tabs own organization | — |
| `thread/goal/updated` | Goal changed | yes | USED: sessionIntent row | adapter |
| `thread/goal/cleared` | Goal changed | yes | USED: sessionIntent row | adapter |
| `thread/queue/changed` | Provider queue changed | no | SKIP: with thread/queue/* | — |
| `project/changed` | Codex app projects | no | SKIP: Domios workspaces are the project model | — |
| `thread/project/updated` | Codex app projects | no | SKIP: Domios workspaces are the project model | — |
| `thread/environment/connected` | Remote execution environments | no | SKIP: Domios runs agents on mesh peers | — |
| `thread/environment/disconnected` | Remote execution environments | no | SKIP: Domios runs agents on mesh peers | — |
| `thread/settings/updated` | Thread settings confirmed | yes | USED: `controlsConfirmed` | adapter |
| `thread/tokenUsage/updated` | Token usage | yes | USED: `usage`; quota display is task_aibmwu | adapter |
| `turn/started` | Turn began | yes | USED: Working; carries reported settings | adapter |
| `hook/started` | Provider hook runs (Domios's own Codex hooks run here) | no | ADOPT → F8 provider hook activity | — |
| `turn/completed` | Turn ended with status completed|interrupted|failed | yes | ADOPTED here: idle + detail interrupted|failed; error row on failure | adapter |
| `hook/completed` | Provider hook runs (Domios's own Codex hooks run here) | no | ADOPT → F8 provider hook activity | — |
| `turn/diff/updated` | Aggregated turn diff | yes | USED: turn diff tool row (Changes view) | adapter |
| `turn/plan/updated` | Plan checklist | yes | USED: update_plan row | adapter |
| `item/started` | Item lifecycle | yes | USED: chat rows; contextCompaction start/complete now drive Compacting (ADOPTED here) | adapter: contextCompaction → compaction item running |
| `item/autoApprovalReview/started` | Guardian auto-approval review | no | ADOPT → F12 auto-review visibility | — |
| `item/autoApprovalReview/completed` | Guardian auto-approval review | no | ADOPT → F12 auto-review visibility | — |
| `autoApprovalReview/strictReviewRequired` | Guardian auto-approval review | no | ADOPT → F12 auto-review visibility | — |
| `item/completed` | Item lifecycle | yes | USED: chat rows; contextCompaction start/complete now drive Compacting (ADOPTED here) | adapter: contextCompaction → compaction completed, then context restore |
| `item/agentMessage/delta` | Streaming text | yes | USED: assistant row deltas | adapter |
| `item/plan/delta` | Streaming text | yes | USED: assistant row deltas | adapter |
| `command/exec/outputDelta` | Standalone exec/process outside a turn | no | SKIP: unsafe second execution path; PTY tabs own shells | — |
| `process/outputDelta` | Standalone exec/process outside a turn | no | SKIP: unsafe second execution path; PTY tabs own shells | — |
| `process/exited` | Standalone exec/process outside a turn | no | SKIP: unsafe second execution path; PTY tabs own shells | — |
| `item/commandExecution/outputDelta` | Command output stream | yes | USED: command tool row result | event logger only |
| `item/commandExecution/terminalInteraction` | Model wrote stdin to a running command | no | COVERED: task_jo0kfs (background terminals) | — |
| `item/fileChange/outputDelta` | Deprecated apply_patch output stream | yes | USED: edit tool row result (kept while still sent) | event logger only |
| `item/fileChange/patchUpdated` | Live structured patch for a fileChange item | yes (this task) | ADOPTED here: updates the edit row's fileDiffs/toolEdits before completion | — |
| `serverRequest/resolved` | Server request settled | yes | USED: `promptResolved` | — |
| `item/mcpToolCall/progress` | MCP tool progress | yes | USED: generic tool row detail | — |
| `mcpServer/oauthLogin/completed` | MCP server OAuth | no | ADOPT later inside F7 MCP server status: one-click re-auth | — |
| `mcpServer/startupStatus/updated` | MCP server status and startup | no | ADOPT → F7 MCP server status | — |
| `mcpServer/event/stream/notification` | Direct MCP calls, resources and event streams (MCP apps) | no | SKIP: Domios renders no MCP apps; revisit after F7 MCP server status | — |
| `account/updated` | Auth changed | yes | USED: `auth` event | — |
| `account/gatewayOAuth/changed` | Gateway OAuth and Bedrock auth | no | SKIP: sign-in capability declares browser, device-code and API key; enterprise auth stays in Codex | — |
| `account/rateLimits/updated` | Rate-limit windows | yes | USED: `rateLimits`; surfaces are task_aibmwu | adapter |
| `app/list/updated` | Codex plugin/app ecosystem | no | SKIP: Domios plugins and channels own extension; installs would mutate the owner's global Codex | — |
| `remoteControl/status/changed` | Codex remote control pairing | no | SKIP: the mesh and web UI are Domios's remote story | — |
| `externalAgentConfig/import/progress` | Import other agents' config | no | SKIP: irrelevant; would rewrite global provider config | — |
| `externalAgentConfig/import/completed` | Import other agents' config | no | SKIP: irrelevant; would rewrite global provider config | — |
| `fs/changed` | Filesystem through the server | no | SKIP: unsafe and redundant; Domios reads files through its tab-scoped reader | — |
| `item/reasoning/summaryTextDelta` | Reasoning streams | yes | USED: thinking row (summary preferred) | adapter |
| `item/reasoning/summaryPartAdded` | New reasoning summary section | yes (this task) | ADOPTED here: opens the summary slot; empty parts never render | — |
| `item/reasoning/textDelta` | Reasoning streams | yes | USED: thinking row (summary preferred) | adapter |
| `thread/compacted` | Deprecated compaction notice (turnId only) | yes | SKIP: deprecated; 0.160.0 does not send it (live capture); contextCompaction item is the signal. Still in the subscription list, remove in F11 | — |
| `model/rerouted` | Server rerouted model | yes | USED: controlsConfirmed + system row | adapter |
| `model/verification` | Moderation metadata, safety buffering, model verification | no | ADOPT → F11 provider notices and safety buffering | — |
| `modelProvider/authRecoveryStarted` | Provider auth recovery | no | ADOPT → F11 provider notices and safety buffering (status detail while recovering) | — |
| `modelProvider/authRecoveryCompleted` | Provider auth recovery | no | ADOPT → F11 provider notices and safety buffering (status detail while recovering) | — |
| `turn/moderationMetadata` | Moderation metadata, safety buffering, model verification | no | ADOPT → F11 provider notices and safety buffering | — |
| `model/safetyBuffering/updated` | Moderation metadata, safety buffering, model verification | no | ADOPT → F11 provider notices and safety buffering | — |
| `warning` | Provider warnings | no | ADOPT → F11 provider notices and safety buffering (system rows) | — |
| `guardianWarning` | Provider warnings | no | ADOPT → F11 provider notices and safety buffering (system rows) | — |
| `deprecationNotice` | Provider warnings | no | ADOPT → F11 provider notices and safety buffering (system rows) | — |
| `configWarning` | Provider warnings | no | ADOPT → F11 provider notices and safety buffering (system rows) | — |
| `fuzzyFileSearch/sessionUpdated` | Fuzzy file search for mentions | no | SKIP: Domios project files and composer references cover mentions provider-neutrally | — |
| `fuzzyFileSearch/sessionCompleted` | Fuzzy file search for mentions | no | SKIP: Domios project files and composer references cover mentions provider-neutrally | — |
| `thread/realtime/started` | Realtime voice events | no | SKIP: with thread/realtime/* | — |
| `thread/realtime/itemAdded` | Realtime voice events | no | SKIP: with thread/realtime/* | — |
| `thread/realtime/item/started` | Realtime voice events | no | SKIP: with thread/realtime/* | — |
| `thread/realtime/item/transcript/delta` | Realtime voice events | no | SKIP: with thread/realtime/* | — |
| `thread/realtime/item/completed` | Realtime voice events | no | SKIP: with thread/realtime/* | — |
| `thread/realtime/transcript/delta` | Realtime voice events | no | SKIP: with thread/realtime/* | event logger only |
| `thread/realtime/transcript/done` | Realtime voice events | no | SKIP: with thread/realtime/* | — |
| `thread/realtime/outputAudio/delta` | Realtime voice events | no | SKIP: with thread/realtime/* | event logger only |
| `thread/realtime/sdp` | Realtime voice events | no | SKIP: with thread/realtime/* | — |
| `thread/realtime/error` | Realtime voice events | no | SKIP: with thread/realtime/* | — |
| `thread/realtime/closed` | Realtime voice events | no | SKIP: with thread/realtime/* | — |
| `windows/worldWritableWarning` | Windows sandbox setup and warnings | no | SKIP now; warnings join F11 provider notices and safety buffering, setup stays in Codex | — |
| `windowsSandbox/setupCompleted` | Windows sandbox setup and warnings | no | SKIP now; warnings join F11 provider notices and safety buffering, setup stays in Codex | — |
| `account/login/completed` | Login finished | yes | USED: `authLogin` event | — |

## Server→client requests (11)

| Method | What it does | Domios uses | Decision | T3 |
|---|---|---|---|---|
| `item/commandExecution/requestApproval` | Approval requests | yes | USED: prompt + needs_input; ADOPTED here: detail awaitingApproval → Permission subtype | adapter |
| `item/fileChange/requestApproval` | Approval requests | yes | USED: prompt + needs_input; ADOPTED here: detail awaitingApproval → Permission subtype | adapter |
| `item/tool/requestUserInput` | Questions and MCP elicitation | yes | USED: prompt + needs_input; ADOPTED here: detail awaitingInput | adapter |
| `mcpServer/elicitation/request` | Questions and MCP elicitation | yes | USED: prompt + needs_input; ADOPTED here: detail awaitingInput | adapter: declines unsupported shapes |
| `item/permissions/requestApproval` | Approval requests | yes | USED: prompt + needs_input; ADOPTED here: detail awaitingApproval → Permission subtype | adapter |
| `item/tool/call` | Client-executed dynamic tool | no | SKIP: Domios registers no dynamic tools; agent tools go through the CLI | — |
| `account/chatgptAuthTokens/refresh` | Server asks client for externally managed tokens | no | SKIP: Codex owns its auth; never sent in our mode | — |
| `attestation/generate` | Server asks for attestation or external clock | no | SKIP: not configured; never sent | — |
| `currentTime/read` | Server asks for attestation or external clock | no | SKIP: not configured; never sent | — |
| `applyPatchApproval` | Legacy approvals for SendUserTurn APIs | no | SKIP: we use turn/start, so v2 approvals arrive instead (T3 still answers both) | adapter |
| `execCommandApproval` | Legacy approvals for SendUserTurn APIs | no | SKIP: we use turn/start, so v2 approvals arrive instead (T3 still answers both) | adapter |

## Client→server notifications (1)

| Method | What it does | Domios uses | Decision | T3 |
|---|---|---|---|---|
| `initialized` | Client ack after initialize | yes | USED: runtime initialize | adapter |

## Proposed follow-up slices (not implemented)

Each slice is provider-neutral: a capability is declared in the manifest and core never branches on provider id. A capability change needs native+JS parity (`parity_test` shared fixture or the provider fixture replay).

| Slice | Capability | Acceptance | Test plan |
|---|---|---|---|
| F1 thread fork and revert | `runtimeOps: fork, revert` → Chat message menu "Fork from here" (new tab, same cwd) and "Revert to here" (same tab, owner-confirmed) | A fork opens a new tab bound to the forked thread; revert truncates history after `thread/reverted` and never replays input | Plugin plan/response fixtures for `thread/fork`, `thread/revert`; reducer case for `thread/reverted`; host test that fork binds a new nonce; parity case |
| F2 provider session name | `sessionName` event → `sessions` list title and tab description; the tab title stays fixed | `thread/name/updated` shows in `sessions search` and Chat header; `thread/name/set` only on an explicit owner rename | Reducer fixture; sessions projection unit test |
| F3 goal control | `goalControl` → composer `/goal set|clear|pause` | Goal set/clear round trip updates the sessionIntent row from `thread/goal/updated` evidence only | Plan fixtures for `thread/goal/set|clear`; reducer fixture |
| F4 review mode | `reviewStart` → composer "Review changes" | `review/start` runs a review turn; entered/exitedReviewMode rows bracket it | Plan fixture; captured review scenario |
| F5 live pickers | `liveCatalogue` (model/list), `collaborationModes`, `permissionProfiles`, provider capability read | Model picker lists the live catalogue (agent models contract: live catalogue defines choices); unknown on failure, never unavailable | Plan/response fixtures; models projection test; parity case |
| F6 account logout and usage read | `login.logout` → `agent logout` and Chat sign-out | Logout signs out without restarting the thread; account/usage/read only if task_aibmwu needs it | Plan fixture; `auth` event fixture |
| F7 MCP server status | `mcpStatus` event → composer strip chip (starting/ready/failed) and Settings provider status | `mcpServer/startupStatus/updated` failure is visible with the server name; re-auth via `mcpServer/oauth/login` is an explicit owner action | Reducer fixture from a live capture (startup frames already appear in every session); UI model test |
| F8 provider hook activity | `hookActivity` rows (quiet) + integration status from `hooks/list` | Domios's own Codex hooks show as collapsed rows; a missing or untrusted Domios hook is reported in Settings | Reducer fixture with `hook/started|completed`; integration status unit test |
| F9 provider skills listing | `providerSkills` → composer `$skill` completion | `skills/list` results appear; `skills/changed` refreshes; no writes to provider skill roots | Plan fixture; completion model test |
| F10 effective config read | `providerConfig` diagnostics → Settings provider status | `config/read` and `configRequirements/read` render read-only; nothing writes global config | Plan fixture; Settings model test |
| F11 provider notices and safety buffering | `notice` system rows + turn detail `checking` for safety buffering and auth recovery | `warning`, `configWarning`, `deprecationNotice`, `guardianWarning`, `model/verification`, `turn/moderationMetadata` render as system rows; `model/safetyBuffering/updated` and auth recovery show a status detail; drop `thread/compacted` from the subscription list | Reducer fixtures; tabActivity label test; parity case for the new detail |
| F12 auto-review visibility | `autoReview` rows | Guardian auto-approval review start/complete and strict-review requirement are visible; `thread/approveGuardianDeniedAction` only as an explicit owner action | Reducer fixtures |
| F13 provider feedback upload | `feedbackUpload` owner action | Owner-confirmed upload from the failure card; never automatic, no logs without consent | Plan fixture; consent gate unit test |
| F14 compaction context completeness | restoreContext `additionalContext` | Any context Domios injects besides the system prompt (role overlays, task files) is restored after compaction; prove with a live long-context capture | Host `compaction_tests` case; captured fixture with auto-compaction |
| F15 binding marker leak | openThread materialization | The "Domios thread binding marker." developer item no longer leaks into replies (the 0.160.0 capture answered with it instead of the requested word) | Captured multi-turn replay asserting the first reply; plan fixture |
| F16 PTY Codex sidecar parity | core `codex_rpc` status path | The PTY Codex sidecar (`service/codex_rpc/status.rs`) maps thread status by Codex method names in core; move it behind the provider's declared roster and add compaction | Rust unit tests in `codex_rpc::status` |
