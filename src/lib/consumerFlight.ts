export function createConsumerFlight<T>() {
  interface Flight {
    controller: AbortController;
    promise: Promise<T>;
    owners: number;
  }
  const flights = new Map<string, Flight>();
  return {
    run(key: string, start: (signal: AbortSignal) => Promise<T>, signal?: AbortSignal): Promise<T> {
      if (signal?.aborted) return Promise.reject(signal.reason);
      let flight = flights.get(key);
      if (!flight) {
        const controller = new AbortController();
        flight = { controller, owners: 0, promise: Promise.resolve().then(() => {
          controller.signal.throwIfAborted();
          return start(controller.signal);
        }) };
        const owned = flight;
        flights.set(key, owned);
        void owned.promise.finally(() => {
          if (flights.get(key) === owned) flights.delete(key);
        }).catch(() => {});
      }
      const owned = flight;
      owned.owners++;
      return new Promise<T>((resolve, reject) => {
        let released = false;
        const release = () => {
          if (released) return;
          released = true;
          signal?.removeEventListener("abort", abort);
          if (--owned.owners === 0) {
            if (flights.get(key) === owned) flights.delete(key);
            owned.controller.abort();
          }
        };
        const abort = () => { release(); reject(signal?.reason); };
        signal?.addEventListener("abort", abort, { once: true });
        owned.promise.then(value => { release(); resolve(value); }, error => { release(); reject(error); });
      });
    },
    cancel() {
      for (const flight of flights.values()) flight.controller.abort();
      flights.clear();
    },
  };
}
