export interface RefreshCoalescer {
  /** Reuse an equal cached generation; otherwise fetch after this call. */
  request(generation?: number): Promise<boolean>;
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
  cachedGeneration: () => number | undefined = () => undefined,
): RefreshCoalescer {
  let running = false;
  let refused = false;
  let waiting: Array<{ generation?: number; resolve: (value: boolean) => void }> = [];

  async function drain(): Promise<void> {
    running = true;
    try {
      while (waiting.length > 0) {
        const batch = waiting;
        waiting = [];
        const needsFetch = batch.some(({ generation }) =>
          generation === undefined || (cachedGeneration() ?? -1) < generation);
        if (!needsFetch) {
          refused = false;
          for (const { resolve } of batch) resolve(true);
          continue;
        }
        let result = false;
        try {
          result = await run();
          refused = false;
        } catch (error) {
          result = false;
          refused = isRefusal(error);
        }
        for (const { resolve } of batch) resolve(result);
      }
    } finally {
      running = false;
    }
  }

  return {
    request(generation) {
      return new Promise<boolean>((resolve) => {
        waiting.push({ generation, resolve });
        if (!running) void drain();
      });
    },
    inFlight: () => (running ? 1 : 0),
    waiting: () => waiting.length,
    lastWasRefused: () => refused,
  };
}
