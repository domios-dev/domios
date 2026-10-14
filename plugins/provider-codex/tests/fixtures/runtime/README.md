# Runtime replay fixtures

These independently authored, sanitized protocol scenarios use the current app-server payload shapes from the plugin and the former runtime source. They are deterministic protocol records, not captures from live agent sessions. No external project content is used.

Each JSON file has this schema (version 1):

```ts
interface RuntimeFixture {
  schemaVersion: 1;
  context: RuntimeContext;
  operations: {operation: Operation; context?: RuntimeContext}[];
  rpc: (
    | {direction: "outbound"; operation: number; action: RuntimeAction}
    | {direction: "inbound"; operation: number; reply: RpcReply}
    | {direction: "inbound"; frame: RuntimeFrame; context?: RuntimeContext}
  )[];
  expected: {
    results: {operation: number; step: RuntimeStep}[];
    events: AgentEvent[];
  };
}
```

`operation` in `rpc` is a zero-based index into `operations`. An operation's context override replaces the top-level context. A frame context override also replaces it, allowing a new runtime epoch with the same thread in the restart scenario. Context includes direct `cwd`, `hostVersion`, and `claimedSends`; it has no environment, token or process handle. All objects use the frozen SDK wire types from `codex-runtime-contract-types.md`.

Replay the `rpc` array in exact order. On the first outbound action for an operation, call `runtimePlan(context, operation)` and compare its action with the recorded action. Preserve the returned plan state. For each corresponding inbound reply, call `runtimeResponse(context, operation, plan.state, reply)`. A `continue` step replaces the plan and the next outbound entry must match its action. A terminal `complete` or `refused` step is appended to `results`. A complete step's optional events are published through the host stamping/validation path, retained in that expected terminal step, and appended to expected.events at the correlated reply position before processing the next RPC entry. expected.events is the complete ordered stream from both runtimeResponse complete.events and runtimeReduce events. A complete step's optional notification is part of that expected step; the harness validates it against the manifest allowlist. The questionAnswer action is host-local formatting (answerQuestion only), carries TurnInput, and is not written to the process; core submits through the ordinary ledger door. Interrupt request calls include generic target metadata validated against activeTurns. A10c threadParents holds retained ancestry independently of the working-turn roster, so inactive parents are traversed but never interrupted. Reply/notification action write acknowledgements use `{kind:"success",value:null}` and do not mean delivery.

For each unsolicited inbound frame, call `runtimeReduce(context, reducerState, [frame])`, initially with `null` state. Preserve the returned state and append returned events in order. Epoch/tab/binding/thread changes reset the replay state to null before invoking the reducer, matching the host harness. Batch tests may combine adjacent frames with identical context and must produce the same events/state. Host parity loads this directory by path and compares independently mapped results/events with `expected`, never using expectations as its adapter implementation.

`RuntimeFrame` is exactly `{kind:"notification",method,params}` or `{kind:"request",requestId,method,params}`. `RpcReply` is exactly `{kind:"success",value}` or `{kind:"error",error:{code,message,data?}}`. These are decoded protocol envelopes: the host strips correlated client IDs from replies, preserves string versus numeric server IDs, and rejects stale epoch/binding input before mapping. Request/reply wire JSON-RPC IDs are represented by the indexed operation correlation or server `requestId`; no synthetic request method represents a server reply.

The eight scenarios cover multi-turn, runtime restart, resume (including archive recovery and pagination), interrupt, interrupt during tool work, steering (including exact refusal fallback), approval/question, and tool call. Expected actions, terminal results, and events are literal independently specified JSON values. History results reconcile items but emit no `sendEvidence`. Live evidence requires a matching current claimed send, exact client ID, text and attachments. Turn acceptance alone supplies no evidence.

Run the built-plugin replay with `pnpm exec tsx plugins/provider-codex/tests/runtime.test.ts`. The provider suite imports this file through `tests/plugin.test.ts`, so `node scripts/run-plugin-tests.mjs provider-codex` also covers every fixture.

Additional captured scenarios and real receipt-rate measurement live in [captured/README.md](captured/README.md). The feature audit and reference approach inventory live in [FEATURE-PARITY.md](FEATURE-PARITY.md).

A9b historyPage.questions contains RuntimeHistoryQuestion records without a kind discriminator. Native async question snapshots are collected across item pages. Only an exact later user answer envelope with matching itemId, question IDs and native option values marks resolved; assistant text and labels do not. History never supplies delivery evidence, and core receipts remain authoritative across separate turns-page operations.

Plugin-generated JSON display strings use recursively lexically sorted object keys, preserve array order, and use fixed indentation. Expected display strings therefore do not depend on Node insertion order or serde_json object ordering. Native source strings are retained verbatim.

A12 launch settings: `settings.additionalDirectories` contains only host-validated absolute paths (max 32, unique, at most 4096 bytes each, no NUL). The plugin maps them to thread config and workspace-write turn policy, preserving the host-authorized permission mode. See [LAUNCH-OPTIONS.md](LAUNCH-OPTIONS.md) for the source inventory, mappings and explicitly refused raw CLI/profile options. Start/resume fixture expectations include the provider's existing update-check and 65536-byte project-instruction defaults.

A12b: optional `settings.networkAccess` is a host-decided boolean, not a CLI flag. It maps to workspace-write thread config and turn policy only. Explicit false is retained; absence leaves Codex defaults untouched.

The resume fixture also replays the Codex 0.160 -32601 item-pagination fallback: thread/read returns the bound snapshot, normalized by the same history item/question path. Its independent expected result preserves open items and pending questions, omits pagination cursor, and emits no delivery evidence. It stays inside the eight shared scenarios so the unchanged host harness exercises this path too. Initialize always declares experimentalApi:true and opts out of no consumed notifications.

A13: resumeThread carries required instructions, forwarded as developerInstructions on the initial resume and the retry after unarchive. restoreContext control accepts {instructions:string|null, additionalContext:[{key,kind:"application"|"user",value:string}]}; instructions/application entries become developer message items, user entries become user message items. Extra fields, malformed entries and duplicate keys are refused. Empty restoration is invalid_schema in the plugin; the host skips that operation before dispatch. Injected context never emits delivery evidence. Focused built-plugin tests cover both resume paths and context roles/validation.
