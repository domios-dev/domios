export async function buildIncrementally<T>(iterator: Generator<void, T>, yieldToHost: () => Promise<void>, signal: AbortSignal, chunkSize = 8): Promise<T> {
  while (true) {
    signal.throwIfAborted();
    for (let i = 0; i < chunkSize; i++) {
      const next = iterator.next();
      if (next.done) return next.value;
    }
    await yieldToHost();
  }
}
