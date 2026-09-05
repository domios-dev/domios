# web-mirror-switch-sync

Does the browser-served web mirror still agree with the daemon after many tabs,
rapid switching, and an `overflow`?

Andrey: *"works disgustingly; it often loses sync with the chat UI when there
are many chats and many switches, especially when working with other machines
through your own."*

## What it runs

The modules under test are the product's own. There is no browser and no
Playwright:

| Piece | Source |
|---|---|
| SSE client, replay cursor, overflow handling, resync fan-out | real `src/lib/webEvents.ts` |
| the store and its generation guard | real `src/store/multiplexer/reducer.ts` |
| relay targeting for the cross-peer origin | real `src/lib/webApi.ts` |
| the ~12 lines of `MultiplexerProvider` wiring between them | modelled in `mirror.ts` (that module is a React component; `looksLikeSnapshot` mirrors `muxStateEventAction`) |

`webEnv.ts` supplies the browser objects those modules need, and owns a gate
that can sever a live event stream - gating `fetch` alone only blocks the next
connect, which is how the first draft of this bench failed to reproduce
anything.

## No sleeps

There is no sleep, fixed delay, or polling interval anywhere in this bench.

- Stream readiness is the SSE response's headers. The daemon subscribes to the
  bus before it answers, so headers are a sound readiness signal.
- A batch of switches is acknowledged by **convergence**: read the generation
  the daemon had already reached once every request answered, then wait for the
  client's store to reach it. Under overlapping switches the daemon coalesces
  and does not emit one `tab-focused` per request, so a per-switch frame is not
  a valid ack.
- The overflow trigger is a **count**, not a duration: park the stream, push
  more events than `RING_MAX_COUNT` (2048) retains, release. The server must
  then classify the client's cursor `Evicted`.
- Every `setTimeout` in the harness is a failure deadline around an event wait.
  Elapsed time is never read as readiness or success.

## Running it

Dev only. Every CLI call goes through `dc`; the harness refuses any origin that
is not port 7686, so prod on 7685 cannot be reached even by accident.

```bash
# local origin
WMS_TABCOUNT=24 WMS_SWITCHES=400 WMS_CONCURRENCY=8 WMS_RUNS=5 \
  evals/web-mirror-switch-sync/run.sh

# cross-peer origin: the browser watching a peer through the mesh web mirror
WMS_PEER_NAME=air WMS_TABCOUNT=24 WMS_SWITCHES=400 WMS_CONCURRENCY=8 \
  evals/web-mirror-switch-sync/run.sh
```

Exit 0 = every run converged; exit 1 = at least one run diverged. Rows land in
`target-check/evals/web-mirror-switch-sync/runs.tsv`.

`WMS_CONCURRENCY` is the knob that matters. At 1 the storm is ack-paced, which
no real user produces; above 1 the switches overlap and the snapshot refetches
they cause overlap with them.

This checkout has no `node_modules`. Either `npm install`, or point
`WMS_TSX_LOADER` at a `tsx/dist/loader.mjs` from a checkout that has one.

## The oracle

`compare()` asserts the client store equals `GET /api/mux/state`: same tab ids,
same focused tab, same titles. A batch that never reaches the daemon's
generation is recorded as `generation-lag`, which is divergence that has not
yet resolved rather than divergence that cannot.

## Reading the results

`commonality.md` holds the column list and the rule for turning rows into a
cause. It was written before any feature was named, and it stays the way a
cause gets named here.
