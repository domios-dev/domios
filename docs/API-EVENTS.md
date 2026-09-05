# Event Stream and Snapshot Load

Contracts the web mirror depends on under load: the full-state snapshot bound and its back-pressure answer. The event stream loss contract itself is in API-STATE (Web Mirror).

### Full-state snapshot overload

`GET /api/mux/state` clones the entire multiplexer under the mux and agent-tracker
locks and serializes it. It is a hot path — the web mirror refetches it on every
switch — and a switch storm on 2026-09-05 drove 1,028-1,050 concurrent snapshots,
after which the daemon aborted twice.

The endpoint is now bounded rather than elastic:

- At most `snapshot_bound::MAX_CONCURRENT_STATE_SNAPSHOTS` snapshots are built at
  any instant. The build itself runs on the blocking pool, never on the async
  runtime, so a slow snapshot cannot starve unrelated IPC.
- A request arriving over the bound is answered `503 Service Unavailable` with the
  body `state snapshot capacity reached`. It is refused at the door — there is no
  queue, no wait, and no delay. The bound is a limit, not a backoff.
- Traffic at or under the bound is unchanged: same body, same status, same
  latency.

**What gets worse.** A client over the bound receives a status it may not handle
today and shows stale state until its next poll succeeds, instead of receiving a
slow snapshot. That is the accepted cost of the daemon surviving the storm. The
alternative — coalescing all waiters onto one in-flight snapshot — was not taken:
it makes every waiter as slow as the slowest build, couples their failures into
one, and still needs a waiter cap to avoid an unbounded queue.

**Clients must treat 503 from this endpoint as retryable-later, not fatal**, and
must not tighten their refetch interval in response to it.

<!-- revision: 1 -->
