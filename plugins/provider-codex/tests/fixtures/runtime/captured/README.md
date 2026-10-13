# Captured Codex 0.160.0 traffic

These additional fixtures were captured directly from the installed `codex app-server` child over stdio, with the catalogue's small `gpt-6-luna` model at low effort. The temporary cwd was deleted after capture. The child received no Domios connection/identity environment. Account response data is projected to account type/plan and authentication requirement only; home paths and cwd are sanitized. No token, email or account identifier is retained.

The main replay schema remains unchanged, with these capture-only additions:

```ts
captured: {cliVersion:string; model:string; scenario:string};
// Each rpc entry additionally has:
receiptMs:number; // monotonic milliseconds since session start
```

Every unsolicited frame has an exact context override. Before a thread is loaded, the context has no threadId and no claimedSends. Each submit operation attaches its exact claim at handoff, and the claim is removed after its first live user item. Parent/active-turn state is host input; the interrupt capture includes its bound active turn. Initialize records its notification write acknowledgement and account/read before the initialized/auth result.

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
