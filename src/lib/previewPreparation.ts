export interface PreviewPreparation {
  state: "ready" | "indexing" | "failed";
  index_id: string;
}

interface Ports {
  prepare(tabId: string): Promise<PreviewPreparation>;
  listen(tabId: string, handler: (state: PreviewPreparation) => void): Promise<() => void>;
  reconnect?(handler: () => void, tabId: string): () => void;
  canReceiveEvents?(tabId: string): boolean;
}

export function createPreviewPreparation(ports: Ports) {
  const pending = new Map<string, Promise<void>>();
  return (tabId: string): Promise<void> => {
    const existing = pending.get(tabId);
    if (existing) return existing;
    const request = (async () => {
      const observed = new Map<string, PreviewPreparation>();
      let expected: string | undefined;
      let settled = false;
      let finish!: () => void;
      let fail!: (error: Error) => void;
      const ready = new Promise<void>((resolve, reject) => {
        finish = () => { settled = true; resolve(); };
        fail = (error) => { settled = true; reject(error); };
      });
      const check = () => {
        const state = expected === undefined ? undefined : observed.get(expected);
        if (state?.state === "ready") finish();
        else if (state?.state === "failed") fail(new Error("Preview reference indexing failed"));
      };
      const unlisten = await ports.listen(tabId, state => {
        if (observed.size === 8) observed.delete(observed.keys().next().value!);
        observed.set(state.index_id, state); check();
      });
      const refresh = async (): Promise<PreviewPreparation> => {
        const state = await ports.prepare(tabId);
        if (expected !== undefined && state.index_id !== expected) throw new Error("Preview session changed");
        expected = state.index_id;
        if (state.state === "ready") finish();
        else if (state.state === "failed") fail(new Error("Preview reference indexing failed"));
        else check();
        return state;
      };
      const disconnect = ports.reconnect?.(() => { void refresh().catch(fail); }, tabId);
      const deadline = setTimeout(() => fail(new Error("Preview reference indexing timed out")), 120_000);
      try {
        let state = await refresh();
        while (!settled && state.state === "indexing" && ports.canReceiveEvents?.(tabId) === false) {
          const next = refresh();
          next.catch(() => {});
          state = await Promise.race([next, ready.then(() => state)]);
        }
        await ready;
      } finally { clearTimeout(deadline); disconnect?.(); unlisten(); }
    })();
    pending.set(tabId, request);
    void request.finally(() => { if (pending.get(tabId) === request) pending.delete(tabId); }).catch(() => {});
    return request;
  };
}
