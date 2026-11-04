# ACP v1 lifecycle fixtures

These are derived, redacted protocol-example transcripts, not live-agent captures.
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
projects a visible truncation marker for a 500,000-character frame and retains
the subsequent healthy chunk. These synthetic stress fixtures share the capture
limitations above; they are not live receipts. Both JS and native replay discover
them through runtime-manifest.json. Native host folding replays their row events
twice to prove exact final content and stable row count.
