export function createChatStartFlight() {
  const active = new Map<string, { key: string; promise: Promise<unknown> }>();
  return <T>(tabId: string, key: string, start: () => Promise<T>): Promise<T> => {
    const current = active.get(tabId);
    if (current) {
      if (current.key === key) return current.promise as Promise<T>;
      return Promise.reject(new Error("Chat attachment already running for this tab"));
    }
    const promise = Promise.resolve().then(start);
    const flight = { key, promise };
    active.set(tabId, flight);
    void promise.finally(() => {
      if (active.get(tabId) === flight) active.delete(tabId);
    }).catch(() => {});
    return promise;
  };
}

export const runChatStart = createChatStartFlight();

export function canRetryChatStart(state: string, cause: string): boolean {
  return state !== "failed" || ["retry", "authority_changed", "history_settled", "resync:epoch_changed"].includes(cause);
}
