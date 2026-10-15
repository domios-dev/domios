export function createProviderCatalogueResource<T>(
  fetch: () => Promise<T>,
  schedule: (callback: () => void) => void = queueMicrotask,
) {
  let response: Promise<T> | undefined;
  let invalidating = false;
  return {
    load(): Promise<T> {
      return response ??= fetch();
    },
    invalidate(): void {
      if (invalidating) return;
      response = undefined;
      invalidating = true;
      schedule(() => { invalidating = false; });
    },
  };
}
