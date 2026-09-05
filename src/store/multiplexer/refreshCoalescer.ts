/**
 * Bounds concurrent full-state refetches to one.
 *
 * `mux-state-changed` on the wire is a bare signal, not a snapshot, so every
 * frame asks for a full `mux_get_state`. Unbounded, a burst opens a fetch per
 * frame: measured at 1028-1050 in flight under rapid tab switching, and the
 * ring replay after one `overflow` adds ~1024 more.
 *
 * A request that arrives while a fetch is running never joins it -- that fetch
 * may have started before the request's own event, so joining could answer
 * with state older than the caller asked about. It waits for the next fetch
 * instead. Every waiter is therefore resolved by a fetch that began at or
 * after its request, and a burst of any size costs at most two round trips.
 *
 * The daemon refuses snapshots past its own concurrency bound (503). That is
 * back-pressure, not failure: it is never retried on a timer or in a loop --
 * the next event's request runs a real fetch, so the refusal cannot become a
 * permanent stale state, and a storm of refusals still costs one fetch at a
 * time.
 */
export interface RefreshCoalescer {
  /** Ask for fresh state. Resolves from a fetch that started after this call. */
  request(): Promise<boolean>;
  /** Fetches in flight: 0 or 1, never more. */
  inFlight(): number;
  /** Requests waiting for the next fetch. */
  waiting(): number;
  /** True when the last attempt was refused by back-pressure rather than
   *  having failed. The next request still runs a fetch either way. */
  lastWasRefused(): boolean;
}

export function createRefreshCoalescer(
  run: () => Promise<boolean>,
  isRefusal: (error: unknown) => boolean = () => false,
): RefreshCoalescer {
  let running = false;
  let refused = false;
  let waiting: Array<(value: boolean) => void> = [];

  async function drain(): Promise<void> {
    running = true;
    try {
      while (waiting.length > 0) {
        // Claim the batch and start the fetch with no await between them, so
        // nothing can slip in and be answered by a fetch older than itself.
        const batch = waiting;
        waiting = [];
        let result = false;
        try {
          result = await run();
          refused = false;
        } catch (error) {
          result = false;
          refused = isRefusal(error);
        }
        for (const resolve of batch) resolve(result);
      }
    } finally {
      running = false;
    }
  }

  return {
    request() {
      return new Promise<boolean>((resolve) => {
        waiting.push(resolve);
        if (!running) void drain();
      });
    },
    inFlight: () => (running ? 1 : 0),
    waiting: () => waiting.length,
    lastWasRefused: () => refused,
  };
}
