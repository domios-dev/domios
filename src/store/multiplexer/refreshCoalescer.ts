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
 */
export interface RefreshCoalescer {
  /** Ask for fresh state. Resolves from a fetch that started after this call. */
  request(): Promise<boolean>;
  /** Fetches in flight: 0 or 1, never more. */
  inFlight(): number;
  /** Requests waiting for the next fetch. */
  waiting(): number;
}

export function createRefreshCoalescer(run: () => Promise<boolean>): RefreshCoalescer {
  let running = false;
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
        } catch {
          result = false;
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
  };
}
