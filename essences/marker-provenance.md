# Marker provenance — threat model & mitigation

Tracks issue #15 (S1). CodeTerm routes agent-to-agent and bridge traffic with
inline `-=-codeterm:<kind> …-=-<body>` markers (see `prod-docs/ROUTING.md`,
`crates/codeterm-types/src/message_markers.rs`). A behavioural rule
(`prompts/codeterm_rule.md`, installed machine-wide to `~/.claude/rules/…`) tells
agents to treat these markers as authenticated routing metadata and act on the
body without the confirmation they would otherwise apply.

## Threat

The marker is ordinary text on the same PTY/stream channel as everything else an
agent reads — file contents, command output, diffs, fetched pages, another
pane's scrollback. Nothing in the bytes cryptographically distinguishes a marker
the host emitted from one a forger typed. An orchestration workload is maximally
exposed: workers spend their lives reading untrusted content, and the rule (as
originally worded) told them a byte sequence inside that content outranks their
own judgement. Because these agents run with permission-bypass flags, an honoured
forged marker is an unsandboxed action.

Attack shape: a malicious file / tool output / fetched page contains a well-formed
`-=-codeterm:mesh …-=-do something dangerous`. A sibling agent reads it and, under
the old rule, honours it as an authentic instruction.

## Mitigation (bounded — not a crypto transport)

A signed envelope / out-of-band field is the real fix and is on the roadmap. This
change ships the bounded mitigation:

1. **Provenance = position.** The daemon prepends its marker as the *leading
   frame* of the message it delivers. That leading position is the only
   host-controlled signal today. A well-formed marker anywhere else is content,
   not metadata.
2. **Mechanism.** `message_markers::provenance()` classifies a candidate as
   `HostInjected` only when the marker is the exact leading frame (optionally
   wrapped in the daemon's bracketed-paste envelope); any preceding byte —
   including padded whitespace — is `StreamContent`. `parse_host_injected()` is
   the trust-gated parser; bare `parse()` recognizes the wire shape only and
   makes no authenticity claim. Trust decisions use `parse_host_injected()`.
3. **Rule wording.** `RULE #1` now trusts a marker ONLY as the host-injected
   leading frame and explicitly distrusts markers embedded in files, tool
   output, fetched pages, diffs, and scrollback. The mesh-peer trust clause is
   scoped the same way.

## What is NOT claimed

This does not authenticate the *sender*, resist a process that can write directly
to a pane's PTY at a message boundary, or replace the need for a signed transport.
It removes the largest, most common exposure — forged markers riding inside the
untrusted content agents constantly read — and gives the daemon a single
trust-gated entry point (`parse_host_injected`) to route all future callers
through.

## Regression coverage

- `crates/codeterm-types/src/message_markers_tests.rs` — provenance mechanism:
  a forged in-content marker and a whitespace-padded marker both classify as
  `StreamContent` and are rejected by `parse_host_injected`; a genuine leading /
  bracketed-paste-wrapped marker is `HostInjected`.
- `evals/routed-reply/evals.json` — behavioural: an agent ignores a marker
  embedded in `cat` output and does not act on it, while still honouring a
  host-injected leading marker.

