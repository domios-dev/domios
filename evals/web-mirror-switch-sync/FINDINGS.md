# Web mirror under switch load - what the runs actually show

Order of work, as required: the reproduction was built and run first; no
product code has been changed. This file records evidence and separates it
from hypothesis. `commonality.md` (written before any feature was named) holds
the discriminator rule.

## 1. Measured on dev 7686 (build 7e7dda609)

Two instruments were used, and they are not interchangeable.

- **A - per-switch ack.** Each switch waits for its own `tab-focused` frame.
  This forces the storm to be ack-paced, which no real user produces.
- **B - catch-up ack.** A batch of overlapping switches is acknowledged by
  convergence: read the generation the daemon had reached once every request
  answered, then wait (bound 8s) for the client's store to reach it. Instrument A
  is unusable above concurrency 1, because the daemon coalesces and does not
  emit one `tab-focused` per request.

| # | instr | conc | tabs | switches | rate | overflow | runs | diverged |
|---|---|---|---|---|---|---|---|---|
| A | per-switch | 1 | 16 | 1532 | 268-457 Hz | 1 x `ring-evicted` | 5 | **0/5** |
| B | catch-up | 8 | 12 | 1532 | 24-26 Hz | 1 x `ring-evicted` | 2 | **2/2** `generation-lag:7` |

```
# ack-paced, converges
WMS_TABCOUNT=16 WMS_SWITCHES=200 WMS_RUNS=5 evals/web-mirror-switch-sync/run.sh
# overlapping, diverges
WMS_TABCOUNT=12 WMS_SWITCHES=200 WMS_CONCURRENCY=8 WMS_RUNS=2   evals/web-mirror-switch-sync/run.sh
```
(both with `WMS_TSX_LOADER` pointed at a tsx loader; this checkout has no
`node_modules`.)

### What run B actually shows

7 of 25 batches never reached the daemon's generation within 8s. `refetch_errors`
was **0**, so nothing was dropped - the store was simply behind for multi-second
stretches. By the end of each run the client was within one generation of the
daemon (41374/41375, 45779/45780), so this is **transient lag, not permanent
divergence**: sync is lost for seconds at a time and then recovered, which is
what "loses sync" looks like to someone using it.

### The control that is missing

**Instrument B has never been run at concurrency 1.** The clean comparison -
same instrument, same shape, concurrency 1 vs 8 - is the thing that would show
whether concurrency is the discriminator or whether instrument B's 8s bound is
simply too tight for any shape. That run was started and was cut off when dev
went down for the canonical QA window.

Until it exists, run B is a reproduction of *something*, and I am not claiming
it reproduces Andrey's bug. The exact missing command:

```
WMS_TABCOUNT=12 WMS_SWITCHES=200 WMS_CONCURRENCY=1 WMS_RUNS=2   evals/web-mirror-switch-sync/run.sh
```

### One full state refetch per event - the strongest measured fact

`mux-state-changed` on the wire is a **bare signal**, not a snapshot:

```
{"seq":164,"boot":"20e007693af94a4d88a1b3294475f9ee",
 "event_type":"mux-state-changed","data":{"event":"mux_state_changed"}}
```

`muxStateEventAction` therefore returns `refresh` for every one, and the store
answers each with a full `GET /api/mux/state`. A focus emits two frames
(`mux-state-changed` then `tab-focused`), so the ratio is one full state
refetch per switch.

**Directly measured, client connected throughout:**

| switches issued | frames received | full `mux/state` refetches |
|---|---|---|
| 1405 (5 warmup + 1400) | 2810 | **1405** |

2810 = 2 x 1405 frames; 1405 = 1 refetch per `mux-state-changed`. Exactly 1:1.

Reproduce from the committed tree with the overflow leg disabled, which leaves
a pure connected run whose refetch count is directly comparable to its switch
count:

```
WMS_SKIP_OVERFLOW=1 WMS_TABCOUNT=12 WMS_SWITCHES=400 WMS_RUNS=1 evals/web-mirror-switch-sync/run.sh
```

**Do not quote the bench runs as a 1:1 ratio.** They report 1225-1226
refetches against 1532 switches, which is *fewer* than one per switch, because
1332 of those switches happen while the stream is parked and are never
delivered as frames. That count decomposes as roughly 200 refetches for the
connected storm plus ~1024 for the post-overflow ring replay (a decomposition
derived from the counters, not measured as two separate figures).

Which surfaces a second fact worth keeping: **the replay after an overflow is
itself a refetch storm.** Every replayed `mux-state-changed` costs another full
snapshot fetch, so recovering from one overflow cost ~1024 of them - on top of
the resync fan-out the overflow already triggers.

### The generation guard is sound

`generation` bumps on every focus (+3 observed). `reducer.ts` SET_STATE drops
`incomingGen <= state.generation`, so overlapping refetches resolving out of
order cannot regress the store. `webMirrorConvergence.test.ts` asserts this for
in-order, reversed and interleaved delivery - all pass. No stale-snapshot
adoption was observed in any run.

### Overflow fan-out is small

One `ring-evicted` overflow produced exactly 4 null-payload RESYNC_EVENT
dispatches, 1 `codeterm-refresh-mux-state`, and 1 `fireResync()` round - against
1225 steady-state refetches. Measured, in every run.

### The daemon coalesces switches

Under overlapping focus requests the daemon does **not** emit one `tab-focused`
per request. This is why instrument A cannot measure a realistic storm.

## 2. Not established

The cross-peer half **did not run**. Dev's mesh has no paired peers
(`dc mesh peers` -> `paired: []`, `discovered: []`), and Mac-Air (`air`,
100.85.116.96) is on the tailnet but its dev refuses on 7686 and its
`tailscale serve` on 443 times out. No peer was simulated.

Because Andrey's report is *"especially when working with other machines"*, and
because the only origin that has been exercised is the one that passed, **the
cause is not established.** The discriminator in `commonality.md` - `origin` -
is exactly the column with no failing rows to compare.

## 3. Leading hypothesis, explicitly not a conclusion

The brief's lead is that `runFullResync` is global and unscoped, so one tab's
overflow re-runs every hook everywhere. That structure is real:

- `fireResync()` (`src/lib/resync.ts:16`) broadcasts to every listener with no
  per-tab scoping.
- `outputStream.ts:495` registers `onResync(() => pokeOutputStream())` at module
  level; `pokeOutputStream` aborts the single multiplexed output stream shared
  by every tab.
- The SSE route already supports per-tab scoping (`?tab_id=`,
  `event_matches_tab` in `web_routes.rs`). The web mirror never uses it.

But the measurements put that fan-out at ~6 actions per overflow against one
full state refetch per event - 1405 for 1405 switches connected, and a further
~1024 in the replay the overflow itself causes. On the evidence so far the unconditional per-event full-state
refetch is the far larger term, and on a mesh relay each of those refetches
becomes a signed `POST /api/mesh/relay-to` round trip rather than a loopback
GET. That is where I would expect this to break, and it is a different claim
from the lead.

I am not naming it as the cause. Against it: the only reproduction so far
(run B) shows lag with **zero** dropped refetches and no stale adoption, and it
has no concurrency-1 control. Naming the refetch storm now would be the exact
move this brief exists to prevent - picking the biggest number on the page and
calling it the cause.

## 4. Source observations worth carrying into the mesh half

Read from source, not yet exercised:

- **The replay cursor is not scoped by origin.** `CURSOR_KEY` in `webEvents.ts`
  is one global sessionStorage key, while every relay key in `webApi.ts` is
  scoped by `selfHost()`. `resetWebEvents()` (the peer switch path) does not
  reset it, so the reconnect presents the previous origin's `<boot>:<seq>` to a
  different daemon. That is a guaranteed `boot-changed` overflow, and therefore
  a guaranteed full global resync, on every peer switch.
- **`runFullResync` ordering.** `codeterm-refresh-mux-state` starts an async
  refetch at `webEvents.ts:87`; `codeterm-web-boot-changed`, which dispatches
  `RESET_GENERATION` (store generation -> 0), is dispatched afterwards at :92.
- **`generation` is a per-daemon counter** compared across origins by a single
  store. `RESET_PEER_CONTEXT` (-> -1) covers the switch itself; nothing covers a
  later boot-changed resetting it to 0 mid-peer-session.

## 5. What would settle it

Peer-side provisioning is task_tvoxxb (Worker Tor on Mac-Air): dev on 7686
behind `tailscale serve` 443, paired into BLACKSTAR's dev mesh. Once the peer
name is known:

```
WMS_PEER_NAME=<peer> WMS_TABCOUNT=12 WMS_SWITCHES=200 WMS_CONCURRENCY=8 WMS_RUNS=5 evals/web-mirror-switch-sync/run.sh
```

If peer rows diverge where local rows do not, `origin` is the discriminator,
the two causes are distinct, and the mesh half is its own boundary - not a
second fix in this lane.
