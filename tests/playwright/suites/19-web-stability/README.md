# 19-web-stability

Canonical web-stability gates for plan `pl_73ee787db5724f0c81c7dc3cebdc8fc5`.

This suite lives outside `12-web` because it is the integration gate layer: tests
compose the lower-level helpers and feature specs into playbook case ids.

## Case Map

| Case | Spec test |
| --- | --- |
| OUT-1 / task_15311c10 | `OUT-1 multiplexed output stream: >=2 terminals use one /api/output/stream and killswitch falls back to per-tab long-poll` |
| RT-1 / Gate 1 | `RT-1/GATE-1 no message loss on reconnect: SSE + output stream sever, mutate, restore, converge` |
| RT-2 | `RT-2 overflow resync: stale event cursor returns explicit overflow/resync frame` |
| RT-3 / Gate 2 | `RT-3/GATE-2 PTY blip survival: tick stream is gapless after output-stream outage` |
| RT-4 / RPL-3 | `RT-4/RPL-3 inject during disconnect is byte-observable with start/end offsets` |
| RT-5 / Gate 2 | `RT-5/GATE-2 visibility return resyncs missed events and post-return updates stay live` |
| RES-1 / GRP-1 / GRP-2 / GRP-3 | `RES-1/GRP-1/GRP-2/GRP-3 restore shape and switching keep per-tab sentinels isolated` |
| GRP-4 | `GRP-4 switching while SSE is severed converges to the latest focused tab` |
| CACHE-1 / CACHE-2 / CACHE-3 | `CACHE-1/CACHE-2/CACHE-3 cached shell paints before delayed mux and reconciles daemon truth` |
| SW-1 / SW-2 | `SW-1/SW-2 offline shell renders cached app while /api is never service-worker cached` |
| NAME-1 / Gate 3 | `NAME-1/GATE-3 tab and workspace names survive reload and daemon save boundary` |
| PAR-1 / MEM | `PAR-1/MEM MemoryTabl web parity: tabl opens without native-only command errors` |
| RPL-1 / RPL-2 | `RPL-1/RPL-2 replay cast renders fidelity sentinels and survives reload` |
| RPL-4 / Gate 4 attention | `RPL-4/GATE-4 attention rail uses hook driver and clears exactly` |
| Gate 4 cross-machine | `GATE-4 cross-machine last-active picker and switched-peer shell via MeshStub` |

RES-2 daemon hard-kill restore is intentionally not duplicated here; it belongs
in `evals/session-restore/` where the harness can own daemon lifecycle without
violating shared-dev process safety.

SW-3 update flow and relay live-stream sub-assertions remain better covered by
their feature-owner specs until the relay implementation exposes deterministic
MeshStub streaming hooks.
