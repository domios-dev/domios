# ACP v1 lifecycle fixtures

Except for the raw-derived dsh scenario below, these are redacted protocol-example transcripts, not live-agent captures.
The checkout had no recorded ACP agent session. No CLI version, receipt time,
model identity or live behavior is claimed. Real agent capture remains required
before claiming live interoperability.

Source: the ACP project's v1 protocol examples and schema:
- https://github.com/agentclientprotocol/agent-client-protocol/blob/main/docs/protocol/v1/initialization.mdx
- https://github.com/agentclientprotocol/agent-client-protocol/blob/main/docs/protocol/v1/session-setup.mdx
- https://github.com/agentclientprotocol/agent-client-protocol/blob/main/docs/protocol/v1/prompt-turn.mdx
- https://github.com/agentclientprotocol/agent-client-protocol/blob/main/schema/v1/schema.json

The fixture envelope normalizes JSON-RPC ids to operation indexes, request/result
frames to runtime actions/replies, and notifications to runtime frames. Cwd,
session ids and tab nonces are replaced with fixture values. Host-only binding
contexts and expected outcomes are adapter assertions, not wire fields.

- `initialize-new-prompt`: v1 initialize, session/new and text prompt examples;
  client filesystem operations and terminal support are explicitly false.
- `unsupported-version`: initialize example with protocolVersion changed to 2.
  Initialization returns a protocol error; subsequent planning returns a local
  refusal because no negotiated epoch exists.
- `load-when-advertised`: loadSession true and the session/load example.
- `resume-when-advertised`: sessionCapabilities.resume object and session/resume,
  followed by a local reattach to the active session. The local action is host
  metadata and emits no ACP frame.
- `resume-not-advertised`: absent optional capabilities; the expected plan error
  proves a returned local refusal action without an outbound session method.
- `cancel`: session/cancel notification targeting the new session. The host active
  turn and local RPC target are adapter authority metadata; no turn id is added
  to ACP wire parameters.
- `foreign-session-frame`: a session/update text chunk with its session id
  changed, followed by an own-session chunk as a positive control.

The root scenario roster is declared in `tests/runtime-manifest.json`; native
and JS replay require the exact discovered set.

Text is baseline ACP v1 prompt content; image, audio and embedded resources are
not sent. Client permission requests use the host approval queue. Filesystem and terminal
requests are delegated to native handlers. Own-session updates use the same stream reducer for live and loaded history.

## Stream scenarios

The stream scenarios are derived, redacted ACP v1 examples under the same source
and capture limitations above. Message IDs are optional extension fields; the
baseline chunks omit them. Consecutive chunk kinds form deterministic rows from
session, prompt-turn ordinal and item ordinal. A user chunk starts the next turn
while loading history. Tool IDs remain provider-supplied identities.

`stream-message-thought-chunks` covers split text, split thoughts and foreign
session rejection. `load-replay-no-duplicates` reattaches twice to that same
conversation in one epoch, with identical final row identities and content.
Direct reducer cases separately prove repeated remote replay preserves those identities.
`tool-call-lifecycle` carries structured oldText/newText diff content and an
explicit result; `cancel-mid-tool` retains and interrupts the active tool card.
`plan-updates` replaces one plan row. `commands-modes-config-usage` retains bounded
command/config/session information in reducer state, publishes the command and
configuration declarations, confirms the mode setting and maps usage and compaction
facts. Notices use the transient runtime event. `unknown-update-kind` emits one
transient warning per kind and epoch, then continues the turn.

`lost-stream` is only the recorded transcript prefix before transport loss. ACP
has no pipe-loss notification, so this fixture intentionally contains no invented
wire frame or successful completion. It proves preservation of the partial row
and absence of a synthesized prompt. Native process exit, visible failure and
bounded recovery require the host process/subscriber tests; JS replay alone does
not establish them.

## Permission and bridge scenarios

The permission scenarios use the same schema-derived provenance, with opaque
option IDs and all four option kinds. `permission-cancelled-on-interrupt` records
the cancellation answer before the host cancellation notification. It proves
adapter mapping; native prompt queue cancellation is a separate host test.

`fs-read-approved`, `fs-write-denied`, and `terminal-lifecycle` prove installed
capability advertisement and that native requests produce no plugin prompt or
execution action. These plugin fixtures do not prove filesystem outcomes or
process retirement: those are covered by `client_bridge::tests` in service-core.

Wire shapes were checked against schema/v1/schema.json on 2026-11-04
(SHA-256 `0d0b1ca83c4872e5473f68926a894c7a8203b59b118e2a79bfe43dd76fbcbc1a`).
No live ACP interoperability or platform execution is claimed by these fixtures.

`tool-call-first-allow` and `tool-call-first-deny` submit a prompt before the first
tool notification. The provider publishes the deterministic turn start before
permission or native filesystem handling. The allow scenario delegates a native
write before tool completion; native approval is still independently required.
`interrupt-before-first-update` keeps the prompt RPC outstanding, sends a
session-scoped cancel without a fabricated active turn, and settles interrupted.

The `interrupt.beforeFirstTurn` declaration is provider-neutral. The host uses it
only with an exact outstanding RPC send from its current epoch; the provider
also refuses cancellation when its own prompt cursor is not in flight.

`controls-selection` derives mode/config request shapes from the same v1 schema
and proves local refusal of unadvertised commands, methods and values.
`declared-auth` proves that only the declared agent method reaches authenticate,
terminal methods stay unavailable, and only a successful authenticate response
can change unknown authentication to authenticated. These remain derived fixtures.

`mcp-fallback-{new,load,resume}-{success,failure}` are derived ACP v1 lifecycle
fixtures: one failed session method with a host-built MCP descriptor retries the
same method once with an empty MCP list. A successful reply binds the session with a notice;
a second failure refuses visibly. Environment credentials use placeholders only.

`mcp-no-fallback-{auth,session-missing,method-not-found}` keep ACP v1 errors
-32000, -32002 and -32601 on their existing typed refusal paths without a retry.
`mcp-fallback-generic-error` proves that a generic -32603 still retries once.


## Bounded stream fixtures

`long-stream-complete` and `long-stream-cancel` extend the schema-derived text
chunk shape to 128 chunks totaling 512 KiB, including non-ASCII text to pin UTF-8
append offsets. Cancellation sends session/cancel while session/prompt is
outstanding, then settles only on its cancelled response. `oversize-single-frame`
retains a 500,000-character frame and its subsequent chunk in full.
`large-single-update` delivers over 200 KiB in one update; `large-multi-update`
delivers multiple large updates ending in a sentinel. Each text update emits one
row event, including the astral boundary case in `oversize-astral-cut`.
`batch-text-1024` restores a mixed drain of small and 8–16 KiB text updates.
`batch-row-determinism` starts a plan, emits more than 32 completed tools with
large Unicode titles, revisits the plan and an evicted tool, then emits implicit
text rows. Its expected events come from individual live frames; the Node harness
also compares those events and row ids against one batched reduction.
`batch-byte-limit` crosses the aggregate serialized-value budget while its input
fits; `batch-event-limit` crosses the event count because a written turn adds
Working activity. Both deliver a subsequent healthy update with the adjusted offset.
Native replay validates each whole reduction under QuickJS and serde. The byte budget
reserves 1 KiB below the host limit for serialization differences. Budgeting runs
once per aggregate; semantic bounds run per frame using cached map byte totals. The Node VM spy limits internal stringify
and UTF-8 sizing work for `batch-text-1024` to four output-length passes plus fixed headroom; this
checks the cost class without claiming a QuickJS timing result.
Text is retained in full within those limits; byte-limit degradation uses a visible
marker and preserves subsequent append offsets. Reducer state retains only identity
and offsets. These synthetic stress fixtures share the capture
limitations above; they are not live receipts. Both JS and native replay discover
them through runtime-manifest.json. Native host folding replays their row events
twice to prove exact final content and stable row count.

The follow-up scenarios remain synthetic, modeled on the run-5 receipt symptoms,
because run 5 did not capture raw ACP stdio frames. two-turns-restart-third preserves
two completed turns across an epoch replacement and uses the restored row ordinal
for a third distinct turn. self-send-overlapping-prompts keeps the first prompt
active until its stopReason, then starts and settles the queued prompt exactly
once, without changing either send's delivery evidence. tool-output-retention
keeps three large tool results and original card identities through a late
completion and interruption of the remaining running cards. Tests also cover an
early queued reply and code-point splitting at an astral boundary. Existing
agent_thought_chunk fixtures remain collapsed thinking rows; the semantic origin
of Clifive's reasoning-like passage remains unverified by the run-5 receipts.

many-turns-tool-admission runs sequential completed tool turns beyond the identity
admission budget. Each new tool remains running until its completion update;
settled identities are reclaimed before admission while running identities remain
protected. The fixture also participates in native/QuickJS shared replay.

planErrors are judged after the whole rpc replay, trailing notifications included. In
model-option-declared-selected the final config_option_update makes wire-one the
authoritative model, so keep that trailing frame when editing the fixture.

## Raw dsh shared message identity

`dsh-shared-message-id.json` derives its two text updates from
`../raw/dsh-reasoning-raw.jsonl`, captured on 2026-11-04 using the installed
`@deepseek-ai/dsh@0.2.1-rc.5` profile and its default `deepseek-v4-flash` model.
Both updates carry messageId `911f94b0-0314-4bb2-afed-a86ec097ae55`:
the thought text and the assistant answer `391` must occupy separate rows.
Only the session id is normalized in those updates. The surrounding lifecycle
uses the existing fixture setup, so this is a derived regression, not a timed
full-session recording. Both native and node shared replay consume it.

Raw JSON-RPC lines are retained in the sibling raw receipts. Credentials were
piped from the named QA instance into the child environment and scrubbed before
writing. The reasoning capture recorded the thought, answer and usage updates;
its reader reached the 90-second bound before recording completion. The smaller
`dsh-raw.jsonl` capture includes an assistant answer and end_turn completion.
Neither receipt by itself proves the repaired application UI.

## Concurrent prompt rejection

`concurrent-rpc-refusal.json` derives A/B/C ordering and replies from
`../raw/dsh-concurrent-refusal-raw.jsonl` (2026-11-04, installed dsh profile,
reasoning Off). Raw reply id 5 rejects B with JSON-RPC code -32602 before reply
id 4 completes A. The capture then includes C's write call, completed update,
`NEXT_TOOL_DONE` answer and end_turn reply. Only session/cwd are normalized in
updates; the fixture uses Domios client identity and host contexts. It does not
infer acceptance from an error message or elapsed time. B reserves ordinal 2,
gets no delivery evidence, and is removed from the queue; C uses ordinal 3 and
settles. Native transcript coverage persists at those settlement events and
restores the complete ordered rows twice without duplicates. The raw probe used
stdin credentials in the child environment, scrubbed before writing, and created
no QA tabs. It proves the wire cause, not the application's repaired live UI.

`active-error-queued-prompt.json` covers an active A prompt returning a correlated
RPC error while B is queued. A's projection is settled and the queued head is
promoted exactly as on success, so B's subsequent updates and completion end at
idle turn 2. A remains a typed Refused result, with no fabricated delivery or
completion events: the host owns refusal settlement and deferred B activity.
