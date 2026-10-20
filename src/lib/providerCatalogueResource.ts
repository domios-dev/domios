export function createProviderCatalogueResource<T>(
  fetch: () => Promise<T>,
  schedule: (callback: () => void) => void = queueMicrotask,
) {
  let response: Promise<T> | undefined;
  let inFlight: Promise<T> | undefined;
  let revision = 0;
  let invalidating = false;
  return {
    load(): Promise<T> {
      if (inFlight) return inFlight;
      if (response) return response;

      const request = new Promise<T>((resolve, reject) => {
        const refresh = async () => {
          let requestedRevision = revision;
          for (;;) {
            try {
              const value = await fetch();
              if (requestedRevision !== revision) {
                requestedRevision = revision;
                continue;
              }
              if (inFlight === request) inFlight = undefined;
              response = request;
              resolve(value);
              return;
            } catch (error) {
              if (requestedRevision !== revision) {
                requestedRevision = revision;
                continue;
              }
              if (inFlight === request) inFlight = undefined;
              if (response === request) response = undefined;
              reject(error);
              return;
            }
          }
        };
        void refresh();
      });
      inFlight = request;
      response = request;
      return request;
    },
    invalidate(): void {
      if (invalidating) return;
      revision++;
      if (!inFlight) response = undefined;
      invalidating = true;
      schedule(() => { invalidating = false; });
    },
  };
}
