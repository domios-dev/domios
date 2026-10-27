# Event Stream and Snapshot Load

Contracts the web mirror depends on under load: published state snapshots and event-driven refresh. The event stream loss contract itself is in API-STATE (Web Mirror).

### Published full-state snapshot

`GET /api/mux/state` returns the latest completed JSON snapshot. A snapshot exists before the first read. Lifecycle writers and snapshot rebuilds do not block requests: readers receive the previous snapshot until the next one is published. Reads do not rebuild or serialize state, and contention does not return `503`.

Genuine storage unavailability still returns `503` with the preservation reason. Keep the current view and surface that reason; never replace it with an empty desk or poll faster.

### Snapshot refetch

`mux-state-changed` is a bare signal, not a snapshot. Publication sends a refresh signal after the new snapshot becomes readable, and only when its content differs from the published one; a rebuild with identical content keeps the published body and its generation and sends nothing. Keep one snapshot fetch in flight; a request arriving mid-fetch waits for the next one, so a burst of any size costs at most two round trips. There is no timer or polling retry.

### Known gap: event order under concurrent publishers

Concurrent publication can omit an event without an `overflow` frame (`rollacode/codeterm#428` family); reconnecting with `since` recovers retained events.

### Live stream census

Authenticated `GET /api/debug/events/stats` includes `streams` alongside the mux emit counters. The census is always enabled, independently of `diagnostics.terminal_transport`.

`emit_causes` reports `requests` and `fires` for each closed cause label (including zero counts). A coalesced fire is attributed once to every cause in its batch, so per-cause fires can sum above `emit_fires`. These counters describe the native mux snapshot emitter, like the existing totals; they do not count SSE publications separately. `emit_unchanged` counts rebuilds whose content matched the published snapshot and therefore sent no signal.

```json
{"streams":{"live":1,"opened":3,"closed":2,"entries":[{"kind":"events","scope":"local","live":1,"opened":3,"closed":2}]}}
```

`entries` groups streams by `kind` (`events`, `output`, `tab_output`, `surface`) and `scope`. A `peer_relayed` entry also has `peer_instance_id`, the remote instance whose stream this daemon relays. Local entries include zero counts; peer entries remain after close so their totals remain available. A relayed connection is counted on the serving daemon as local and on the proxy daemon as peer relayed; sample both daemons separately.

`live` counts open response streams, including bodies that have not yet been polled. Completion, client disconnect, cancellation and panic unwind release the count. `opened` and `closed` are monotonic within the daemon process and reset on restart. A snapshot reads atomic counters independently; concurrent opens/closes can briefly make `opened - closed` differ from `live`. After stream owners finish, `live` returns to the baseline and opened/closed deltas balance. Finite JSON and asset responses are outside the SSE census.

### CI failure events

POST `/api/ci/webhook` uses a dedicated Bearer credential from the secret-store entry `ci-webhook`, a JSON registration `{token, repo, plan}`. It accepts `{repo, run, attempt, sha, job, step, test, exit, log_url}`; `test` and `exit` may be null when unavailable. Run and attempt are positive decimal strings, SHA is 40 hex characters, and the log URL must name that repository/run/attempt on GitHub. Authenticated events route one message and notification to the registered plan's live, session-verified Orchestrator. Missing owners fail delivery; events never fall back to another tab. Duplicate repository/run/attempt/job events are suppressed by the gateway's bounded process-local cache. Requests are capped at 16 KiB and retain the API's Tailnet access restrictions.


## Additional REST routes

Blueprint scope and payload contracts: BLUEPRINTS. Notification controls: API-TABS.

| Method | Path | Operation |
|--------|------|-----------|
| DELETE | `/api/automation/blueprints/{id}` | Delete a Blueprint. |
| DELETE | `/api/automation/sentinels/{id}` | Delete a Blueprint (sentinel route alias). |
| DELETE | `/api/notifications/{id}` | Delete one notification. |
| DELETE | `/api/sentinels/{id}` | Delete a Blueprint (sentinel route alias). |
| GET | `/api/automation/graph` | Read the scoped Blueprint graph snapshot. |
| GET | `/api/automation/graph/workspace` | Read the scoped Blueprint graph snapshot. |
| POST | `/api/automation/blueprints/solutions/archive` | Archive a saved Blueprint solution. |
| POST | `/api/automation/blueprints/solutions/list` | List saved Blueprint solutions. |
| POST | `/api/automation/blueprints/solutions/save` | Save a Blueprint solution. |
| POST | `/api/automation/blueprints/{id}/stop` | Stop a Blueprint. |
| POST | `/api/automation/node-files` | Manage Blueprint node files. |
| POST | `/api/automation/sentinels/{id}/stop` | Stop a Blueprint (sentinel route alias). |
| POST | `/api/ci/webhook` | Receive authenticated CI failure events; credential and limits above. |
| POST | `/api/notifications/{id}/read` | Mark one notification read. |
| POST | `/api/repo-watch/subscribe` | Subscribe to repository changes. |
| POST | `/api/repo-watch/unsubscribe` | Unsubscribe from repository changes. |
| POST | `/api/sentinels/{id}/stop` | Stop a Blueprint (sentinel route alias). |

<!-- revision: 10 -->
