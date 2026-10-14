# Codex runtime feature inventory

Source audit: the local reference's `apps/server/src/orchestration-v2/Adapters/CodexAdapterV2.ts`, `apps/server/src/provider/CodexProvider.ts`, `CodexThreadRevert.ts`, and `codexUsageLimits.ts`; Domios `src-tauri/src/service/codex_rpc` at the lane base. This inventory records observed source support; execution evidence is the replay suite and three captured 0.160.0 sessions, not an assertion that every row was exercised live. No reference code, fixture content or license text was copied.

| Method or notification | Purpose | Existing codex_rpc | Plugin mapping | Gap / proposed followup |
|---|---|---|---|---|
| initialize / initialized | Experimental handshake | Yes | Request plus post-success notification | Host owns one guarded initialization and process lifetime |
| thread/start | New thread with launch settings | Yes | openThread, verified root identity | Host provides composed instructions and exact cwd |
| thread/inject_items | Materialize thread / restore instructions after compaction | Yes | openThread continuation; restoreContext control | Host routes contextCompacted to restoreContext once with original context |
| thread/resume | Load exact saved thread | Yes | excludeTurns, exact identity, archive recovery | No fresh-thread fallback |
| thread/unarchive | Recover archived session | Yes | Correlated resume refusal continuation | Bounded host operation |
| thread/read | Verify identity without history | Yes | resumeThread verification | Account/login status is separate below |
| thread/turns/list | Paginated turns | Yes | readHistory page and repeated cursor rejection | Host drains returned nextCursor |
| thread/items/list | Paginated items per turn | Yes | readHistory continuations and openItems | Host reconciles stable IDs and unfinished items |
| turn/start | Submit a new turn | Yes | submitTurn with clientUserMessageId, settings, attachments | Exclusive ledger remains host-owned |
| turn/steer | Add input during active turn | Yes | submitTurn with expectedTurnId; exact refused-before-acceptance fallback | Diverges from reference by attaching our exclusive send ID; never retry uncertainty |
| turn/interrupt | Stop running turn | Yes, root plus descendants | Root and verified active descendants, deepest first | A10/A10b/A10c generic targets are validated; retained ancestry traverses inactive parents |
| thread/settings/update | Next-turn model/effort/mode | Old core stages locally | Per-thread control request | Never writes machine config |
| thread/compact/start | Compact active context | Not exposed as RPC control | compact control | Completion emits contextCompacted |
| turn/started | Activity and provider start time | Yes | turnState with startedAt in ms | Auto-started goal turns on exact bound thread accepted |
| turn/completed | Finish, error, provider completion time | Yes | Deduplicated turnState + system error item | Old completed turn cannot settle a newer one |
| thread/status/changed | Activity / approval wait | Yes | working, idle, needs_input | No timer or terminal inference |
| item/started / item/completed | Stable chat and tool snapshots | Yes | User, assistant, reasoning, command, file diff, MCP/dynamic tool, web/image, subagent, compaction, review, plan, intent | Captured user/assistant/command paths tested |
| item/agentMessage/delta | Streaming assistant text | Yes | Stable item text accumulation | No batching timer; already-queued batches remain ordered |
| item/reasoning/summaryTextDelta | Summary parts | Yes, concatenated | Independent indexed summary stream | Does not blend raw reasoning into summary |
| item/reasoning/textDelta | Raw reasoning parts | Yes, concatenated | Independent indexed content stream | Summary remains display preference |
| item/plan/delta | Proposed plan text | Yes | Assistant plan item | Uses same stable item mechanism |
| turn/plan/updated | Todo / plan status | Yes | update_plan tool snapshot with full steps | Existing renderer consumes toolInput/result |
| turn/diff/updated | Unified turn patch | Opted out | ChatMsg.fileDiffs + exact raw patch | includeTurnDiffs provider setting opts in; default preserves old/reference handshake |
| item/commandExecution/outputDelta | Tool stdout | Yes | Append toolResult | Terminal snapshot replaces authoritative output |
| item/fileChange/outputDelta | Edit output | Yes | Append toolResult | Exact file edits use recorded changes |
| item/mcpToolCall/progress | Tool progress | Yes | Update toolDetail | No guessed completion |
| thread/tokenUsage/updated | Tokens and context usage | No runtime normalization | usage counters including cache write/read | Observed 0.160.0 cacheWriteInputTokens mapped |
| account/rateLimits/updated | Account allowance | No runtime mapping | Separate rateLimits event; sparse merge, main allowance isolation | A8; no guessed tokens or durations |
| thread/settings/updated | Confirm model/effort/mode | Yes | controlsConfirmed from native settings | Host confirms selection using fresh owned event |
| model/rerouted | Provider selected a different model | Not mapped | controlsConfirmed + system notice | Native from/to/reason retained in toolInput |
| item/commandExecution/requestApproval | Command decision | Yes | prompt + exact server reply; native decisions retained | Captured decline round trip; structured amendment decisions preserved |
| item/fileChange/requestApproval | Edit decision | Yes | prompt + exact server reply | No guessed approval authority |
| item/permissions/requestApproval | Turn/session permissions | Yes | scope-bound prompt + exact reply | Never grants automatically |
| item/tool/requestUserInput | Blocking questions | Yes | Exact native option values and answers keyed by question ID | Multi-question free text UI requires richer answer shape; one free-text question works |
| mcpServer/elicitation/request | MCP form/URL elicitation | Limited empty form | Existing empty-form prompt/reply retained | Rich forms and URL launch require UI/contract decision |
| serverRequest/resolved | Remove live request | Yes | promptResolved with exact ID | String and numeric IDs remain distinct |
| async agentMessage.questions | Nonblocking questions | Not supported | question event, response message, native values; answerQuestion formatting action; typed historyPage.questions | A9; durable itemId resolution and ordinary send ledger belong core |
| thread/goal/updated | Provider goal text | Yes | Stable sessionIntent item | Durable goal lifecycle/commands deliberately deferred |
| thread/goal/cleared | Clear provider goal | Not mapped | Clear stable sessionIntent item | Same thread accepts subsequent goal-native turns |
| thread/goal/get | Read current goal during history | Yes, optional | Optional final-page history enrichment to sessionIntent | Unsupported optional probe does not fail history |
| thread/goal/set / clear | Autonomous goal commands | No | Deferred | Owner decision required; goal state/UI/run ownership |
| error | Retry/failure notification | Yes | System row with classified metadata | Retry notices never settle turn or replay payload |
| account/read / account/login/completed | Authentication observation | Yes | initialize/account-read and auth refresh emit auth; account/updated maps directly | A11/A11b maps auth without account identifiers |
| account/rateLimits/read | Usage refresh | Existing provider fetchUsage surface | Existing provider usage probe unchanged | Runtime account windows arrive through notification |
| model/list | Live model catalogue | Existing provider discoverModels | Existing plugin discovery unchanged | Outside pure thread hooks |
| skills/list | Skill discovery | No runtime call | Deferred | Host skill capability unchanged |
| mcpServerStatus/list | MCP server inventory | No | Deferred | Requires management surface/typed result |
| mcpServer/tool/call / resource/read | Explicit MCP invocation | No | Deferred | Requires tool management operation; ordinary tool events already mapped |
| thread/backgroundTerminals/list / terminate | Background terminal control | No | Deferred | Requires owned-process/task UI and capability |
| execCommandApproval / applyPatchApproval | Legacy protocol approvals | No | Deferred | CLI compatibility requires current 0.159/0.160 API; do not revive legacy adapters |
| thread/unsubscribe | Drop subscription | Existing client lifetime only | Host runtime teardown | No provider protocol shutdown hook in frozen API |
| thread/fork | Fork at native turn boundary | No | Deferred | Needs thread identity/result and UI action |
| thread/revert | Roll back to boundary | No | Deferred | Reference pages history to beforeTurnId; never send obsolete numTurns |
| feedback/upload | Submit feedback | No | Deferred | Requires explicit user flow |

## Approach provenance and deliberate differences

| Piece | Reference counterpart read | Adaptation and reason |
|---|---|---|
| Stable notification/item mapping | CodexAdapterV2.ts item/started and item/completed handlers | Same source item IDs and authoritative final snapshots; use shared ChatMsg instead of orchestration execution nodes |
| Assistant/plan deltas | CodexAdapterV2.ts agentMessageDeltas / planDeltas | Accumulate per stable item; no timer coalescer because our contract forbids timer-driven coordination |
| Reasoning | CodexAdapterV2.ts appendReasoning / completeReasoning | Separate indexed summary/content; shared thinking row prefers summaries rather than separate execution nodes |
| Approval/input | CodexAdapterV2.ts approval and blocking input handlers; docs/internals/providers.md Protocol traps | Preserve native decisions/values, request IDs and scopes; use generic PendingPrompt instead of reference runtime request registry |
| Async questions | CodexAdapterV2.ts async completed agentMessage branch | Emit persisted question with response message; core formats and deduplicates ordinary user sends through its ledger |
| Interrupt/completion races | CodexAdapterV2.ts turn/completed and interruptingNativeTurns | Native completion owns idle; late completion cannot settle newer turn; no protocol acceptance is called delivery |
| Steering | CodexAdapterV2.ts steerTurn | expectedTurnId retained; Domios exact correlated pre-acceptance refusal additionally permits one new-turn fallback, required by its exclusive ledger |
| Token usage | CodexAdapterV2.ts codexProviderTurnTokenUsage | Cumulative counters plus last-turn context window; no reference orchestration accounting model copied |
| Rate limits | codexUsageLimits.ts mergeCodexRateLimits / codexRateLimitsToWindows | Merge sparse main allowance snapshots; preserve reported durations only instead of guessing plan-dependent durations |
| JSON display | CodexAdapterV2.ts dynamic tool JSON presentation | Recursively sorted keys make Node and the serde/QuickJS bridge render identically; source string results remain verbatim |
| Errors | CodexAdapterV2.ts error handler | Retain retryable/class metadata on stable system row; avoid classifying a retry notice as terminal failure |
| Timing | CodexAdapterV2.ts codexTimestamp / turn handlers | Seconds to ms, host stamps receipt time; plugin never reads a clock |
| Resume/archive/history | CodexAdapterV2.ts resumeThread; CodexThreadRevert.ts pagination | excludeTurns and archive recovery retained; no rollback/fork action implemented without approved UI/result contract |
| Host process/guarded initialize | effect-codex-app-server client and provider managed runtime | Owned by core/Bridge lanes; no process/FS/env/network access in these hooks |

Canonical must regenerate the shared InteractiveOption generated TypeScript after the Rust value field lands. This lane fills source native values structurally and never edits generated artifacts.

Codex 0.160 experimental history divergence: installed app-server acknowledges experimentalApi=true yet thread/items/list returns -32601 "not supported yet". On that correlated method-not-found response only, history falls back to the exact bound thread/read includeTurns:true snapshot, following T3 CodexAdapterV2.ts readThreadSnapshot. It reuses item/question normalization, validates thread identity and complete turn/item arrays, never emits sendEvidence, and remains subject to host snapshot size bounds. Other RPC errors remain failures. All consumed notifications, including turn/diff/updated, stay subscribed.

Effective settings: start/resume replies report model and reasoningEffort separately from the requested values. Following T3 CodexAdapterV2.ts decodeCodexChildModel/updateSubagentSelection and the old Codex lifecycle effective settings, these replies override the request only for reported model/effort/mode; omitted values retain the request. Continuation state carries them through materialization/identity verification. Controls replies and settings notifications share the same decoder. Host-authorized directories/network settings are retained, never inferred from provider permission output.

Resume/context instruction parity: T3 CodexAdapterV2.ts explicitly refreshes runtime policy on resumed turns rather than relying on sticky persisted values. Domios refreshes its full host-composed developer instructions on thread/resume, including unarchive retry. RestoreContext follows the approved A13 host payload and its exact developer/user role mapping; this deliberately differs from T3's application-context policy because the frozen Domios contract owns these roles. No I/O or additional-context producer is added to pure hooks, and an empty restore is rejected because the host skips it.

Settings-update acknowledgement: the live Codex 0.160 endpoint returns success with an empty result. The approved control contract treats that correlated success as acknowledgement of the requested settings; reported effective values override the request, errors never confirm. Bound thread settings/model reroute/turn-start notifications still reconcile later native substitutions. This corrects the earlier empty-reply policy while keeping the one request/one response path and existing host controls projection.
