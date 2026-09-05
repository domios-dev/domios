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

**Superseded in part - read section 1b before quoting the 2/2.** The same
shape converged on the next build, so this is an intermittent reproduction,
not a deterministic one.

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

### The control that was missing (now run - see 1b)

At the time of writing, **instrument B had never been run at concurrency 1.** The clean comparison -
same instrument, same shape, concurrency 1 vs 8 - is the thing that would show
whether concurrency is the discriminator or whether instrument B's 8s bound is
simply too tight for any shape. That run was started and was cut off when dev
went down for the canonical QA window.

It has since run: 0/2 diverged (section 1b). That is the comparison this
paragraph asked for, and it is also why the concurrency-8 result needs a
failure rate on a single build before `concurrency` can be called the
discriminator. The command:

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

## 1b. Gate results on build 190b80704 - and a correction

Gates 2 and 3 ran on the rebuilt dev. They change the picture.

| shape | conc | tabs | switches | rate | overflow | maxInFlight | slowest refetch | runs | diverged |
|---|---|---|---|---|---|---|---|---|---|
| gate 3, no overflow leg | 1 | 12 | 400 | 72 Hz | 0 | - | - | 1 | 0/1 |
| gate 2, control | 1 | 12 | 1532 | 208-211 Hz | 1 | - | - | 2 | **0/2** |
| same shape as the failures | 8 | 12 | 1532 | 144 Hz | 1 | **1027** | 1684 ms | 1 | **0/1** |

**Gate 3 confirms the 1:1 ratio from the committed tree:** 400 switches ->
427 refetches. The 27 extra are background events from unrelated tabs, which
is what one-refetch-per-`mux-state-changed` predicts.

**Gate 2, the control, converged 0/2** at concurrency 1 with the same
instrument, same tab count, same switch count and a real `ring-evicted`
overflow. Elapsed 7.3 s per run against 59-62 s for the concurrency-8 runs.

### The correction

On build 7e7dda609 the concurrency-8 shape diverged **2/2**. On build
190b80704 the same shape **converged 0/1**. So the earlier report's
"diverges 2/2" is not a property of the shape: **the reproduction is
intermittent and did not survive a rebuild.** One pass is thin evidence, but
it is enough to retract "deterministic".

Current honest tally for the concurrency-8 shape: **2 failures out of 3 runs,
across two different builds** - and the two failures and the one pass are not
on the same build, so they are not one sample. The failure rate on
190b80704 is unmeasured; the run intended to measure it was cut off when dev
went down.

Consequence: `concurrency` looked like the discriminator against the control,
and it may still be, but it cannot be called one on this evidence. No cause is
named, and the brief's lead is still not the local explanation either -
`overflow_fired` is 1 in the passing control *and* in the failing runs, so
the overflow fan-out cannot by itself separate them.

### One robust mechanism number

Independent of pass or fail, the concurrency-8 run measured **1027 concurrent
`GET /api/mux/state` requests in flight at peak**, with the slowest single
refetch taking 1684 ms. That is the per-event refetch with no coalescing and
no in-flight dedup, under overlapping switches. It is a fact about the client
regardless of which way a given run's assertion lands, and it is the number I
would carry into the mesh half, where each of those is a signed relay round
trip instead of a loopback GET.

## 1c. Build c9245a513: the sync bug does not reproduce, the daemon crash does

Five runs requested at concurrency 8. Four completed; the fifth killed the
daemon.

| run | rate | overflow | refetch | maxInFlight | slowest refetch | diverged |
|---|---|---|---|---|---|---|
| 1 | 177 Hz | 1 | 1275 | 1050 | 2090 ms | no |
| 2 | 202 Hz | 1 | 1240 | 1028 | 1921 ms | no |
| 3 | 157 Hz | 1 | 1238 | 1031 | 1947 ms | no |
| 4 | 166 Hz | 1 | 1237 | 1029 | 2182 ms | no |
| 5 | - | - | - | - | - | **daemon died mid-storm** |

### The sync divergence is build-specific

`generation-lag` now stands at **0/4 on c9245a513** plus 0/1 on 190b80704:
**0/5 across the two newer builds**, against 2/2 on 7e7dda609. The
concurrency-8 shape is not a reproduction of Andrey's bug on current code. The
retraction in 1b stands and hardens: whatever those two failures were, they do
not survive either rebuild, and `concurrency` is not established as the
discriminator.

### The daemon crash does reproduce

Twice, on two different builds:

| when | build | dump | teardown in the window? |
|---|---|---|---|
| 11:22:28Z | 190b80704 | yes, `0xc000001d` (Rust abort) | yes - `workspace close --force` on 12 PTYs |
| run 5, ~11:27:4xZ | c9245a513 | **no dump written** | **no** |

The second death isolates it. It happened inside `Promise.all` on a concurrent
focus batch (`ECONNRESET` on a `POST .../focus`), with no `workspace close
--force` anywhere in the window - runs 2 through 5 tear nothing down between
them. **The switch storm alone can take the daemon out.** The forced 12-PTY
teardown, the other candidate from the first crash, is not required.

Two differences worth keeping rather than smoothing over: the first death
wrote a minidump and the second wrote none, and `.clean_exit` is stale
(10:52) in both cases, so neither was a clean shutdown. A crash reporter that
catches one and misses the other may be two failure modes or one racing its
own reporter. Not established.

### What is solid across every run

`maxInFlight` sits at **1028-1050 concurrent `GET /api/mux/state`** in every
single concurrency-8 run, with the slowest single refetch 1.9-2.2 s. That is
the most stable number this bench produces, it is the per-event refetch with
no coalescing and no in-flight dedup, and it is present in the runs that pass
as well as the ones that kill the daemon.

Tracked as https://github.com/rollacode/codeterm/issues/428.

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

## 4b. The peer-switch sequence, read and verified

`WebPeerPicker.switchTo` (`src/components/WebPeerPicker/WebPeerPicker.tsx:166`)
is the whole origin switch, in order:

1. `invalidateWebRequestContext()`
2. `codeterm-peer-switch-reset` -> `RESET_PEER_CONTEXT` (store generation -> -1)
3. `setRelayTarget()` / `clearRelayTarget()`
4. `invalidateWebRequestContext()`
5. `resetWebEvents()`
6. `codeterm-peer-switch-rebind` -> `refreshFromBackend()`

Two things are missing from that list, and both are verifiable without a peer.

**`resetOutputStream()` is dead code.** It has exactly one reference in the
repository - its own definition at `src/lib/outputStream.ts:498`:

```
$ grep -rn "resetOutputStream" --include=*.ts --include=*.tsx .
./src/lib/outputStream.ts:498:export function resetOutputStream(): void {
```

Its doc comment says *"Tear down (peer switch) - next subscription reconnects
to the current host."* No peer switch calls it. So across an origin switch the
multiplexed PTY output stream is never torn down: its connection to the
previous origin stays open, and its `offsets` and `replayed` maps stay keyed by
the previous origin's tab ids. `outputStreamUrl()` reads `relayTarget()` only
when it (re)connects, and nothing makes it reconnect.

**The replay cursor is not reset either.** `resetWebEvents()` clears the
watchdog, the abort controller and `started`, but not the module-level
`cursor`. `CURSOR_KEY` is a single unscoped sessionStorage key, while every
relay key in `webApi.ts` is scoped by `selfHost()`.

## 4c. Falsifiable predictions for the cross-peer run

Written before the peer exists, so the peer rows decide rather than illustrate.
If a prediction fails, the observation above is wrong and I say so.

- **P1.** Each origin switch produces exactly one `boot-changed` overflow,
  because the reconnect presents the previous origin's `<boot>:<seq>` to a
  different daemon. Measure: `overflow_fired` and `bootChanges()` per switch.
- **P2.** Terminal output does not follow the origin switch: the output stream
  stays attached to the previous origin. Measure: whether output frames for
  the new origin's tab ids ever arrive without an unrelated reconnect.
- **P3.** Steady-state refetch cost per event is unchanged in shape but each
  refetch is a `POST /api/mesh/relay-to` round trip rather than a loopback
  GET. Measure: `mux_refetches` against wall time, peer vs local.

P1 and P2 follow from 4b and need only an origin switch. P3 is the term that
would make the local-vs-mesh answer "same cause, worse constant"; P1 and P2
would make it "distinct causes", which is a boundary, not a second fix here.

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
