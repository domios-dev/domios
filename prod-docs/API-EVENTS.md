# Event Stream and Snapshot Load

Contracts the web mirror depends on under load: the full-state snapshot bound and its back-pressure answer. The event stream loss contract itself is in API-STATE (Web Mirror).

### Full-state snapshot overload

`GET /api/mux/state` is bounded. A request over the bound, or one arriving while a lifecycle change is being written, is answered `503 Service Unavailable` with the body `state snapshot capacity reached`, at once: no queue, no wait. Traffic under the bound is unchanged.

**Clients must treat 503 from this endpoint as retryable-later, not fatal**, and must not tighten their refetch interval in response to it.

### Snapshot refetch

`mux-state-changed` is a bare signal, not a snapshot. Keep one snapshot fetch in flight; a request arriving mid-fetch waits for the next one, so a burst of any size costs at most two round trips. After a 503 the client stays not-yet-reconciled and the next `mux-state-changed` or `mesh-state-changed` event schedules one read. There is no timer or polling retry.

### Known gap: event order under concurrent publishers

Concurrent publication can omit an event without an `overflow` frame (`rollacode/codeterm#428` family); reconnecting with `since` recovers retained events.

<!-- revision: 4 -->
