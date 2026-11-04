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
- `resume-when-advertised`: sessionCapabilities.resume object and session/resume.
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
not sent. Client permission requests, filesystem and terminal methods are outside
this lifecycle surface. The reducer emits only the lifecycle working state for
own-session agent chunks; stream projection is a separate task.
