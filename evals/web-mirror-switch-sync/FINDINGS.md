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

## 1d. The client fix: bounding the refetch storm

Per the Orchestrator's ruling, the storm is a defect on its own numbers and
the client half belongs to this lane. The daemon half (aborting under load
instead of back-pressuring) is a separate Rust task on #428 and is not
touched here.

**Owning boundary.** All 24 call sites in `src/store/multiplexer/index.tsx`
reach the daemon through one function, `refreshFromBackend`. The fix is a gate
inside it, not a change at any call site.

`src/store/multiplexer/refreshCoalescer.ts` allows one fetch in flight. A
request arriving while a fetch is running never joins it -- that fetch may
have started before the request's own event, so joining could answer with
state older than the caller asked about. It waits for the next fetch instead.

The contract, asserted in `refreshCoalescer.test.ts` on fetch counts:

- a burst of 200 requests costs **2** fetches, never 2 at once. Not 1: the
  first request cannot be answered by a fetch older than itself, so it opens
  one immediately and the other 199 share the follow-up. Writing the test
  expecting 1 is how that was found.
- a sustained storm of 1000 requests across 20 rounds stays at **one** fetch
  in flight throughout, at one fetch per round plus one.
- no waiter is ever answered by a fetch that started before its request.
- a thrown fetch resolves `false` rather than rejecting, and does not wedge
  the queue.

This covers the overflow replay storm by the same mechanism: the ~1024
replayed `mux-state-changed` frames now collapse to at most two fetches.

`mirror.ts` routes the bench through the product's coalescer rather than a
model of it, and the bench now fails a run whose `max_in_flight` exceeds 1
(`in-flight-ceiling:N`), so the authorized run proves the ceiling on the wire
rather than in a unit test alone.

**Not claimed:** that this fixes Andrey's reported sync symptom. It does not
address it, and 0/5 on current code says the local shape does not reproduce
that symptom at all. This bounds a defect the bench found, at its own
boundary.

## 1e. The per-tab output oracle, and why the mux oracle was the wrong surface

Andrey's words were "loses sync with the CHAT UI". Every result above compares
the mux store: tab set, focused tab, titles. None of them ever looked at what
is behind a tab. `outputStream.ts`, `remoteChannelRecovery.ts` and
`useChatSession` are in the assigned surface and the bench exercised none of
them.

That is a plausible reason the local shape reads 0/5: a tab list can stay
perfectly converged while the terminal content behind it is stale or missing,
and the second is what a user calls losing sync with the chat.

`outputOracle.ts` subscribes through the product's own `subscribeTabOutput`
and asks a different question: after a switch storm, does every tab's output
that the daemon produced actually reach the client?

**No PTY input is written.** Each tab prints its marker from its own startup
command (`dc tab new --cmd "echo WMSMARK-..."`). PTY input delivery is a
fragile path gated on explicit approval, and the output path is what this
oracle is about; `POST /api/pty/{id}/write` is deliberately not used.

The storm supplies the pressure without any input: a tab's first subscription
this page lifetime sets its offset to 0 and forces the single shared
multiplexed stream to reconnect (`subscribeTabOutput` -> `forceReconnect`), so
rapid switching tears that one connection down repeatedly while every other
tab is still streaming through it.

A missing marker is only a mirror bug if the daemon actually has it, so
`run.sh` cross-checks each reported loss with `dc tab get-text` and prints
`MIRROR LOSS` or `not a mirror loss` per tab. Row columns: `output_tabs`,
`output_missing`, `output_truncations`; verdict `output-loss:N`.

Not yet run: it needs a dev window. If it fails where the mux oracle passed,
that is the first real local reproduction and the discriminator list gets its
first honest failing row.

### A correction to what I reported earlier

I told the Orchestrator the bench "fails any run whose `max_in_flight`
exceeds 1". It did not. The `IN_FLIGHT_CEILING` constant was committed but the
verdict never used it -- an edit whose anchor silently missed, and my
verification grepped the constant's declaration rather than its use. The
assertion is real now (`in-flight-ceiling:N`), and the lesson is that grepping
for a symbol proves it exists, not that anything calls it.

## 1f. THE CAUSE: seq allocation and bus publish are not atomic

Found in Tor's captures, not in my bench. Two 60 s captures of the same
27-tab workspace, same harness, no `tab_id` filter on either.

| leg | source build | frames | mux-state-changed | silent seq gaps |
|---|---|---|---|---|
| peer (Mac-Air client, BLACKSTAR source) | b9ba09cf3 | 9739 | 4994 | **9** |
| local (Mac-Air client and source) | 190b80704 | 2184 | 585 | **0** |

Nineteen seqs are absent from the peer leg. Ten of them (21462-21471) are
bounded by the overflow's `next_seq: 21472` -- signalled loss, working as
designed. The other **nine are silent**: 12436, 15256, 15418, 15435, 15436,
18252, 18266, 18415, 18594.

Every one of the nine is:
- inside a single connection (no reconnect, no `stream_error`),
- with **no overflow frame anywhere near it**,
- with its two neighbours timestamped **0.0 ms apart** -- they arrived in the
  same 64 KB read,
- and the capture recorded **zero** `malformed` frames, so nothing was
  mis-parsed.

The relay is exonerated: `mesh_relay_stream` is
`Body::from_stream(resp.bytes_stream())`, a pure byte pipe. Byte truncation
would corrupt a frame into a `malformed` record, not cleanly remove one while
its neighbours arrive intact in the same chunk.

### The mechanism

`EventBus::send_bus` (`src-tauri/src/api/events.rs:276`):

```rust
let seq = self.ring.lock().push(event.clone(), Instant::now());
let _ = self.system_bus.send(SeqEvent { seq, event });
```

The seq is allocated under the ring lock; the temporary guard is dropped at the
end of that statement, so the broadcast `send` happens **outside** it. Two
concurrent publishers can therefore interleave:

```
A: lock -> seq = N   -> unlock
B: lock -> seq = N+1 -> unlock
B: system_bus.send(N+1)      <-- reaches the bus first
A: system_bus.send(N)
```

and the live loop in `events_stream` (`web_routes.rs`) then does:

```rust
if se.seq <= replayed_max { continue; }   // N <= N+1 -> dropped, silently
replayed_max = se.seq;
```

N is discarded with no overflow frame. **The design's stated invariant --
"Loss has exactly one signal: overflow" -- is violated.** The ring still holds
N in order, so a reconnect with `?since=` recovers it; until something forces
one, the mirror is silently short one event and diverges.

Corroboration: both captures are **strictly increasing** in seq with zero
decreasing steps. That is exactly what this guard predicts -- a reordered
lower seq is never delivered late, it is dropped, so the loss is invisible in
the stream itself and only shows as a hole. The double gap at 15435-15436 is
a three-way interleave.

### The discriminator is event RATE, not origin

This is where I have to be careful, because the tempting read is "the mesh
loses frames" and the data does not support it. The peer leg carried 9739
frames to the local leg's 2184 in the same 60 s -- about 4.5x the publish
rate -- and the two legs also ran different daemon builds. So peer-vs-local is
confounded with both load and build. What the mechanism predicts, and what the
data shows, is that the loss scales with **concurrent publishes**, which is a
rate effect that the peer path amplifies rather than causes.

That matches the report better than a mesh-specific theory does: more chats and
more switching means more concurrent publishes means more reorders, and the
cross-machine path is worse because relaying a busy peer multiplies the event
volume the client sees.

### Ownership

This is the daemon half, in Rust, and it is not mine to fix: the client lane
owns the mirror. Filed against #428's family, at
`src-tauri/src/api/events.rs:276` (allocate and publish atomically) with
`web_routes.rs`'s live-loop guard as the place the loss becomes silent.

My client-side coalescer does not address this and never could: it bounds how
many snapshots the client asks for, not which events the daemon delivers.

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
