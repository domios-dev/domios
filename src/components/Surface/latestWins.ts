export interface LatestWins<T> {
  push(item: T): void;
  dispose(): void;
}

export function createLatestWins<T>(
  run: (item: T) => Promise<void>,
  onError: (error: unknown) => void = () => undefined,
): LatestWins<T> {
  let busy = false;
  let disposed = false;
  let pending: { item: T } | null = null;

  const drain = async () => {
    busy = true;
    while (pending && !disposed) {
      const { item } = pending;
      pending = null;
      try {
        await run(item);
      } catch (error) {
        onError(error);
      }
    }
    busy = false;
  };

  return {
    push(item) {
      if (disposed) return;
      pending = { item };
      if (!busy) void drain();
    },
    dispose() {
      disposed = true;
      pending = null;
    },
  };
}
