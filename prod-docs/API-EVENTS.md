# Event Stream and Snapshot Load

Contracts the web mirror depends on under load: published state snapshots and event-driven refresh. The event stream loss contract itself is in API-STATE (Web Mirror).

### Published full-state snapshot

`GET /api/mux/state` returns the latest completed JSON snapshot. A snapshot exists before the first read. Lifecycle writers and snapshot rebuilds do not block requests: readers receive the previous snapshot until the next one is published. Reads do not rebuild or serialize state, and contention does not return `503`.

Genuine storage unavailability still returns `503` with the preservation reason. Keep the current view and surface that reason; never replace it with an empty desk or poll faster.

### Snapshot refetch

`mux-state-changed` is a bare signal, not a snapshot. Publication sends a refresh signal after the new snapshot becomes readable. Keep one snapshot fetch in flight; a request arriving mid-fetch waits for the next one, so a burst of any size costs at most two round trips. There is no timer or polling retry.

### Known gap: event order under concurrent publishers

Concurrent publication can omit an event without an `overflow` frame (`rollacode/codeterm#428` family); reconnecting with `since` recovers retained events.

### Live stream census

Authenticated `GET /api/debug/events/stats` includes `streams` alongside the mux emit counters. The census is always enabled, independently of `diagnostics.terminal_transport`.

```json
{"streams":{"live":1,"opened":3,"closed":2,"entries":[{"kind":"events","scope":"local","live":1,"opened":3,"closed":2}]}}
```

`entries` groups streams by `kind` (`events`, `output`, `tab_output`, `surface`) and `scope`. A `peer_relayed` entry also has `peer_instance_id`, the remote instance whose stream this daemon relays. Local entries include zero counts; peer entries remain after close so their totals remain available. A relayed connection is counted on the serving daemon as local and on the proxy daemon as peer relayed; sample both daemons separately.

`live` counts open response streams, including bodies that have not yet been polled. Completion, client disconnect, cancellation and panic unwind release the count. `opened` and `closed` are monotonic within the daemon process and reset on restart. A snapshot reads atomic counters independently; concurrent opens/closes can briefly make `opened - closed` differ from `live`. After stream owners finish, `live` returns to the baseline and opened/closed deltas balance. Finite JSON and asset responses are outside the SSE census.

<!-- revision: 6 -->
