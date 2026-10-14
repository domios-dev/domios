# Captured Codex 0.160.0 traffic

These additional fixtures were captured directly from the installed `codex app-server` child over stdio, with the catalogue's small `gpt-6-luna` model at low effort. The temporary cwd was deleted after capture. The child received no Domios connection/identity environment. Account response data is projected to account type/plan and authentication requirement only; home paths and cwd are sanitized. No token, email or account identifier is retained.

The main replay schema remains unchanged, with these capture-only additions:

```ts
captured: {cliVersion:string; model:string; scenario:string};
// Each rpc entry additionally has:
receiptMs:number; // monotonic milliseconds since session start
```

Every unsolicited frame has an exact context override. Before a thread is loaded, the context has no threadId and no claimedSends. Each submit operation attaches its exact claim at handoff, and the claim is removed after its first live user item. Parent/active-turn state is host input; the interrupt capture includes its bound active turn. Initialize records its notification write acknowledgement and account/read before the initialized/auth result. The auth response event is included in the full expected.events stream as well as the expected terminal step.

`multi-turn.json`, `interrupt.json`, and `approval.json` prove the two-turn conversation, interrupted native completion, and blocking command approval round trip (Decline). `streaming-short.json` is the requested approximately 400-word no-tool streaming session. It completed in less than ten seconds, so it cannot establish a sustained ten-second rate. `streaming.json` is an additional 800-word no-tool session that provides a complete observed ten-second interval. Both are real traffic, not generated load.

The actions and inbound decoded envelopes are recorded from the child. Expected results and events are independently specified from those envelopes; the oracle generation never invokes the hooks. Timestamps, final text, user client IDs, available decisions and tool outputs are source values. Replays compare the built hooks with the recorded actions and expected results/events.

Measure rate with:

```bash
node plugins/provider-codex/tests/runtime-metrics.mjs plugins/provider-codex/tests/fixtures/runtime/captured/streaming.json
```

The script scans complete windows bounded by recorded receipt times, counting notifications only. Peak is the largest observed one-second rate; sustained is the largest complete ten-second rate. Mean frame size is UTF-8 bytes of the compact notification wire form `{method,params}`. The benchmark threshold is twice the measured sustained rate. No idle extension or invented receipt timestamp contributes to the measurement.

Manual live recapture spends provider tokens and is not a test-suite gate:

```bash
node plugins/provider-codex/tests/capture-runtime.mjs /path/to/temporary-output multi-turn interrupt approval streaming
```

The recorder has a single bounded per-session failure deadline. It waits on stdout frames and correlated replies, with no sleeps or readiness polling. Inspect/redact the resulting account response projection and author expected results/events before installing a capture fixture. All approvals in the capture exercise are declined.

## Recorded T3 subagent traffic

`subagent.json` and `subagent_v2.json` project only collaboration/activity item notifications from the reference client's recorded Codex 0.156.1 app-server traffic (`apps/server/src/orchestration-v2/testkit/fixtures/subagent/codex_transcript.ndjson` and `subagent_v2/codex_transcript.ndjson`). Their source is the local `refs/reference` reference checkout at `8ac4b97dae6bff7258fa5091a908116902fbd635`; no reference implementation code was copied. Wire parameters, IDs, prompts, models, efforts and timestamps are retained verbatim. `receiptMs` is the recorded `emittedAtMs` relative to the first selected notification, rather than a new capture timestamp. Host context is a synthetic binding to the recorded root thread with no send claims.

Expected Chat rows were specified independently from the envelopes. The first fixture proves two receiver identities retain prompt/model/effort/ordinal while wait results update those identities. The second proves v2 activity uses one stable child row across separate lifecycle item IDs; a receiver-less wait stays a generic tool card. Both replay through the existing built-plugin Node and host QuickJS fixture harnesses. These recordings do not prove background-terminal cleanup or child transcript navigation.

The usage handshake replays add an `account/rateLimits/read` adapter step after recorded subscription account reads. Its response is the separately captured, redacted `../../runtime-rate-limits-read.json` snapshot. Those added steps have no receipt timestamp and do not claim to have occurred in the original conversation capture. Auth expected events now retain the billing mode from the recorded account type.
