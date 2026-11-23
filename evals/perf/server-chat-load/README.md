# Server CPU under chat and viewer load

Launch a disposable named DEV on a private port using the canonical build owner.
Pin its full build SHA. Use a quiet instance with no existing viewer, agent
workload or owner data. The harness installs a unique in-process fixture plugin
with no host I/O, account, subprocess or network capability. All turns route to
that plugin. It closes and verifies the fixture workspaces and removes the plugin.

```bash
CODETERM_INSTANCE=gauge-load CODETERM_API_PORT=17741 \
  bash evals/perf/server-chat-load/run.sh \
  --sha FULL_DEV_SHA --out target-check/evals/server-chat-before --agents 8
```

Use the same arguments with the new build SHA and a fresh output directory for
the after run. Set `--batches` to change the number of live deltas per agent,
`--history` to change the number of historic rows seeded into the viewed tab,
or repeat `--phase` to select rows such as `0-off` and `0-on`. Each row starts
from fresh tabs. It ends on delivery of the exact final event from each agent.
One local observer consumes all chat events in every row to establish the fence.
The viewer opens a separate subscription and reads a cold chat snapshot before
the measured turns. The snapshot must contain tool cards and the viewer must
receive every expected live delta for the selected tab.

By default the viewer uses the daemon's tab-scoped SSE route directly. Include
`--mesh-gateway GATEWAY_INSTANCE:GATEWAY_PORT` to exercise the
signed relay stream and snapshot on a second pre-paired, same-build named DEV
on the same host. The runner verifies the gateway's pairing against the
target DEV's mesh identity and reports both daemons' HTTP counters.

The 2- and 5-client rows send bounded waves to the owner-authenticated
`/api/mesh/sync` route once per 30 live deltas from the selected tab. This
measures synthetic sync traffic with separate client IDs. Peer signing,
membership checks, real gossip and multi-machine behavior require a live mesh
topology. The fixture exercises provider polling, chat admission, transcript
storage and the common event stream. Codex app-server parsing and native
runtime projection require their own fixtures.

Each row writes JSON with CPU time and average cores from the OS process
counters for the exact listener PID, working set at the start and end, and
private bytes on Windows. Route counts and body bytes come from the daemon's
DEV census. Latency is client wall time through complete response consumption.
Event frames, bytes and overflow come from live SSE, plus the daemon's event
encoding and retention counters. The Windows connection snapshots group
established inbound TCP sockets by source address without retaining addresses.
Embedding queue snapshots mark background indexing that can confound CPU
comparisons. Compare rows after the queue drains when measuring viewer effects.
Process CPU describes overall cost. Route latency and event bytes measure the
request and stream workload. A profiler can attribute CPU time to call sites.
